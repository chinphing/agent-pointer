//! DashScope native video understanding via OpenAI-compatible `video_url` input.

use crate::llm_token_stats::LlmUsageSnapshot;
use crate::media::token::{record_media_understand_usage, MediaTokenContext, MediaUnderstandKind};
use crate::media::video::{
    dashscope_clamp_fps, video_data_url_encoded_len, MAX_VIDEO_API_BASE64_BYTES,
};
use crate::models::{
    provider_uses_dashscope_compatible_api, AgentModelRef, ModelSettings, ProviderConfig,
};
use crate::provider::ChatOnceOutput;
use anyhow::{anyhow, Context, Result};
use base64::Engine;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const VIDEO_UNDERSTAND_PROMPT: &str = "Analyze the attached video according to the user's goal. \
Describe scenes, actions, visible text, and key details relevant to the goal.";

use crate::text_util::truncate_bytes;

fn resolve_provider_api_key(settings: &ModelSettings, api_key_fallback: &str) -> String {
    let pid = settings.active_provider_id.trim();
    if let Some(p) = settings.providers.iter().find(|p| p.id == pid) {
        if !p.api_key.trim().is_empty() {
            return p.api_key.clone();
        }
        return String::new();
    }
    api_key_fallback.trim().to_string()
}

fn video_data_url(mime_type: &str, bytes: &[u8]) -> String {
    let mime = mime_type.trim();
    let mime = if mime.is_empty() { "video/mp4" } else { mime };
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{mime};base64,{b64}")
}

fn build_compatible_video_request(model: &str, video_url: &str, fps: f64, goal: &str) -> Value {
    json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": VIDEO_UNDERSTAND_PROMPT
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "video_url",
                        "video_url": { "url": video_url },
                        "fps": fps
                    },
                    {
                        "type": "text",
                        "text": goal
                    }
                ]
            }
        ],
        "stream": false
    })
}

fn extract_compatible_text(payload: &Value) -> Result<String> {
    if let Some(content) = payload
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
    {
        let t = content.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    anyhow::bail!("compatible video response missing text");
}

fn extract_compatible_usage(payload: &Value) -> Option<LlmUsageSnapshot> {
    let usage = payload.get("usage")?;
    let input = usage
        .get("prompt_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let output = usage
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let total = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(input as u64 + output as u64) as u32;
    Some(LlmUsageSnapshot {
        prompt_tokens: input,
        completion_tokens: output,
        total_tokens: total,
        reasoning_tokens: 0,
    })
}

async fn understand_video_dashscope_compatible(
    provider: &ProviderConfig,
    api_key: &str,
    model_id: &str,
    video_data_url: &str,
    fps: f64,
    goal: &str,
    cancel: &CancellationToken,
) -> Result<ChatOnceOutput> {
    let url = format!(
        "{}/chat/completions",
        provider.base_url.trim_end_matches('/')
    );
    let body = build_compatible_video_request(model_id, video_data_url, fps, goal);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .context("dashscope video http client")?;

    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(anyhow!("cancelled")),
        r = client
            .post(&url)
            .bearer_auth(api_key.trim())
            .json(&body)
            .send() => r?,
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("HTTP {}: {}", status, truncate_bytes(&text, 400));
    }

    let parsed: Value = resp
        .json()
        .await
        .context("parse dashscope video response")?;
    let text = extract_compatible_text(&parsed)?;
    let usage = extract_compatible_usage(&parsed);
    Ok(ChatOnceOutput {
        text,
        usage,
        model: model_id.to_string(),
        tool_calls: vec![],
        reasoning_content: None,
        finish_reason: None,
    })
}

/// Native DashScope video via `video_url` (Base64 data URL). Returns `Ok(None)` when provider is not DashScope-compatible.
pub async fn understand_video_dashscope(
    settings: &ModelSettings,
    video_model: &AgentModelRef,
    api_key_fallback: &str,
    video_bytes: &[u8],
    mime_type: &str,
    goal: &str,
    fps: f64,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<Option<String>> {
    if video_data_url_encoded_len(mime_type, video_bytes.len()) >= MAX_VIDEO_API_BASE64_BYTES {
        log::warn!(
            "media_understand video: {} bytes exceeds DashScope Base64 limit ({}); skip native path",
            video_bytes.len(),
            MAX_VIDEO_API_BASE64_BYTES
        );
        return Ok(None);
    }
    let data_url = video_data_url(mime_type, video_bytes);
    understand_video_dashscope_url(
        settings,
        video_model,
        api_key_fallback,
        &data_url,
        video_bytes.len(),
        "Base64",
        goal,
        fps,
        token_ctx,
        cancel,
    )
    .await
}

/// Native DashScope video via HTTP or data `video_url` (OSS presigned URL or Base64).
pub async fn understand_video_dashscope_url(
    settings: &ModelSettings,
    video_model: &AgentModelRef,
    api_key_fallback: &str,
    video_url: &str,
    payload_hint_bytes: usize,
    input_mode: &str,
    goal: &str,
    fps: f64,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<Option<String>> {
    let mut video_settings = settings.clone();
    if !video_model.provider_id.trim().is_empty() {
        video_settings.active_provider_id = video_model.provider_id.trim().to_string();
    }
    if !video_model.model.trim().is_empty() {
        video_settings.model = video_model.model.trim().to_string();
    }
    let api_key = resolve_provider_api_key(&video_settings, api_key_fallback);
    if api_key.is_empty() {
        anyhow::bail!("no API key for video understanding model");
    }
    let Some(provider) = video_settings
        .providers
        .iter()
        .find(|p| p.id == video_settings.active_provider_id)
    else {
        return Ok(None);
    };
    if !provider_uses_dashscope_compatible_api(provider) {
        return Ok(None);
    }

    let url = video_url.trim();
    if url.is_empty() {
        anyhow::bail!("native video_url is empty");
    }

    let fps = dashscope_clamp_fps(fps);
    log::info!(
        "media_understand video: native video_url input ({input_mode}, ~{} bytes, fps={fps}, model={})",
        payload_hint_bytes,
        video_settings.model.trim()
    );

    let out = understand_video_dashscope_compatible(
        provider,
        &api_key,
        video_settings.model.trim(),
        url,
        fps,
        goal,
        cancel,
    )
    .await
    .context("dashscope native video understand")?;
    record_media_understand_usage(token_ctx, MediaUnderstandKind::Video, &out);
    let text = out.text.trim().to_string();
    if text.is_empty() {
        anyhow::bail!("native video understanding returned empty content");
    }
    Ok(Some(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_uses_video_url_and_fps() {
        let body = build_compatible_video_request(
            "qwen3.5-flash",
            "data:video/mp4;base64,abc",
            1.0,
            "Summarize the clip",
        );
        let content = body["messages"][1]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "video_url");
        assert_eq!(content[0]["fps"], 1.0);
        assert!(content[0]["video_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:video/mp4;base64,"));
        assert_eq!(content[1]["text"], "Summarize the clip");
    }

    #[test]
    fn parses_compatible_video_text() {
        let payload = json!({
            "choices": [{
                "message": { "content": "A person walks across the room." }
            }]
        });
        assert_eq!(
            extract_compatible_text(&payload).unwrap(),
            "A person walks across the room."
        );
    }
}
