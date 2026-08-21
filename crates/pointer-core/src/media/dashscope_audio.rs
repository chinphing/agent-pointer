//! DashScope APIs for speech-to-text (Feishu voice, IM audio, etc.).

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::llm_token_stats::LlmUsageSnapshot;
use crate::media_generation::dashscope_multimodal_image_url;
use crate::models::ProviderConfig;
use crate::provider::ChatOnceOutput;
use crate::text_util::truncate_bytes;

/// Use the catalogued speech model as configured. Do not remap by name.
pub fn dashscope_audio_model_id(configured: &str) -> &str {
    configured.trim()
}

fn uses_qwen_asr_api(model_id: &str) -> bool {
    model_id.to_ascii_lowercase().contains("asr")
}

pub async fn transcribe_audio_dashscope_multimodal(
    provider: &ProviderConfig,
    api_key: &str,
    model: &str,
    audio_data_url: &str,
    cancel: &CancellationToken,
) -> Result<ChatOnceOutput> {
    let model_id = dashscope_audio_model_id(model);
    if uses_qwen_asr_api(model_id) {
        return transcribe_audio_dashscope_compatible(
            provider,
            api_key,
            model_id,
            audio_data_url,
            cancel,
        )
        .await;
    }
    transcribe_audio_dashscope_legacy_multimodal(
        provider,
        api_key,
        model_id,
        audio_data_url,
        cancel,
    )
    .await
}

/// Qwen3-ASR-Flash via OpenAI-compatible `chat/completions` + `input_audio`.
async fn transcribe_audio_dashscope_compatible(
    provider: &ProviderConfig,
    api_key: &str,
    model_id: &str,
    audio_data_url: &str,
    cancel: &CancellationToken,
) -> Result<ChatOnceOutput> {
    let url = format!(
        "{}/chat/completions",
        provider.base_url.trim_end_matches('/')
    );
    let body = json!({
        "model": model_id,
        "messages": [{
            "role": "user",
            "content": [{
                "type": "input_audio",
                "input_audio": {
                    "data": audio_data_url
                }
            }]
        }],
        "stream": false,
        "asr_options": {
            "enable_itn": false
        }
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .context("dashscope asr http client")?;

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

    let parsed: Value = resp.json().await.context("parse dashscope asr response")?;
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

/// Legacy qwen-audio-turbo via `multimodal-generation` + `audio` content part.
async fn transcribe_audio_dashscope_legacy_multimodal(
    provider: &ProviderConfig,
    api_key: &str,
    model_id: &str,
    audio_data_url: &str,
    cancel: &CancellationToken,
) -> Result<ChatOnceOutput> {
    let url = dashscope_multimodal_image_url(&provider.base_url);
    let body = json!({
        "model": model_id,
        "input": {
            "messages": [{
                "role": "user",
                "content": [
                    {"audio": audio_data_url},
                    {"text": "请将这段音频转写为简体中文文本，只输出转写内容，不要其他说明。"}
                ]
            }]
        }
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .context("dashscope audio http client")?;

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
        .context("parse dashscope audio response")?;
    let text = extract_multimodal_text(&parsed)?;
    let usage = extract_multimodal_usage(&parsed);

    Ok(ChatOnceOutput {
        text,
        usage,
        model: model_id.to_string(),
        tool_calls: vec![],
        reasoning_content: None,
        finish_reason: None,
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
    anyhow::bail!("compatible audio response missing text");
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
        cached_tokens: 0,
    })
}

fn extract_multimodal_text(payload: &Value) -> Result<String> {
    if let Some(choices) = payload
        .pointer("/output/choices")
        .and_then(|v| v.as_array())
    {
        for choice in choices {
            if let Some(content) = choice
                .pointer("/message/content")
                .and_then(|v| v.as_array())
            {
                for item in content {
                    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                        let t = text.trim();
                        if !t.is_empty() {
                            return Ok(t.to_string());
                        }
                    }
                }
            }
            if let Some(text) = choice.pointer("/message/content").and_then(|v| v.as_str()) {
                let t = text.trim();
                if !t.is_empty() {
                    return Ok(t.to_string());
                }
            }
        }
    }
    anyhow::bail!("multimodal audio response missing text");
}

fn extract_multimodal_usage(payload: &Value) -> Option<LlmUsageSnapshot> {
    let usage = payload.get("usage")?;
    let input = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let output = usage
        .get("output_tokens")
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
        cached_tokens: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_model_id_is_kept() {
        assert_eq!(dashscope_audio_model_id("qwen3.5-flash"), "qwen3.5-flash");
        assert_eq!(dashscope_audio_model_id("fun-asr"), "fun-asr");
        assert_eq!(
            dashscope_audio_model_id("qwen3-asr-flash"),
            "qwen3-asr-flash"
        );
    }

    #[test]
    fn audio_model_kept() {
        assert_eq!(
            dashscope_audio_model_id("qwen-audio-turbo-latest"),
            "qwen-audio-turbo-latest"
        );
    }

    #[test]
    fn parses_multimodal_text_array() {
        let payload = json!({
            "output": {
                "choices": [{
                    "message": {
                        "content": [{"text": "你好"}]
                    }
                }]
            }
        });
        assert_eq!(extract_multimodal_text(&payload).unwrap(), "你好");
    }

    #[test]
    fn parses_compatible_text() {
        let payload = json!({
            "choices": [{
                "message": {
                    "content": "你好世界"
                }
            }]
        });
        assert_eq!(extract_compatible_text(&payload).unwrap(), "你好世界");
    }
}
