//! DashScope **Generation API** web search for generic `web_search` tool calls.

use crate::models::{
    effective_web_search_model, find_dashscope_provider, ModelSettings, DEFAULT_WEB_SEARCH_MODEL,
};
use anyhow::{anyhow, Context, Result};
use log::{info, warn};
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use super::client::{
    build_tool_generation_stream_request_body, dashscope_native_generation_url,
    log_web_search_http_request, parse_search_response, resolve_web_search_api_model,
    truncate_for_log, DashScopeSearchConfig, WebSearchRequest, WebSearchResult,
    DEFAULT_WEB_SEARCH_TIMEOUT_SECS,
};
use super::sse_drain::{read_dashscope_search_sse, read_dashscope_search_sse_from_str};
use super::stream_ui::WebSearchStreamUi;

/// Default model for Generation + `enable_search` (tool path).
pub const DEFAULT_TOOL_WEB_SEARCH_MODEL: &str = "qwen3-max";

pub fn resolve_dashscope_generation_config(
    settings: &ModelSettings,
    agent_id: Option<&str>,
) -> Result<DashScopeSearchConfig> {
    let provider = find_dashscope_provider(settings).ok_or_else(|| {
        anyhow!(
            "No DashScope-compatible provider configured. Add one in settings before using web search."
        )
    })?;

    let api_key = provider.api_key.trim();
    if api_key.is_empty() {
        return Err(anyhow!(
            "DashScope provider API key is missing. Configure the provider API key in settings."
        ));
    }

    let configured = effective_web_search_model(settings, agent_id);
    let model = resolve_tool_generation_model(&configured);

    Ok(DashScopeSearchConfig {
        api_key: api_key.to_string(),
        generation_url: dashscope_native_generation_url(&provider.base_url, &model),
        model,
    })
}

fn resolve_tool_generation_model(configured: &str) -> String {
    let m = configured.trim();
    if m.is_empty() || m == DEFAULT_WEB_SEARCH_MODEL {
        return DEFAULT_TOOL_WEB_SEARCH_MODEL.to_string();
    }
    let resolved = resolve_web_search_api_model(m);
    if resolved == DEFAULT_WEB_SEARCH_MODEL {
        DEFAULT_TOOL_WEB_SEARCH_MODEL.to_string()
    } else {
        resolved
    }
}

async fn parse_generation_json_fallback(text: &str) -> Result<Value> {
    serde_json::from_str(text).context("DashScope web search returned invalid JSON")
}

pub async fn execute_generation_web_search(
    settings: &ModelSettings,
    agent_id: Option<&str>,
    req: WebSearchRequest,
    cancel: CancellationToken,
    ui: Option<WebSearchStreamUi>,
) -> Result<WebSearchResult> {
    let config = resolve_dashscope_generation_config(settings, agent_id)?;
    let body = build_tool_generation_stream_request_body(&config.model, &req);
    log_web_search_http_request(&config.generation_url, &body, true);
    let started = Instant::now();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(DEFAULT_WEB_SEARCH_TIMEOUT_SECS))
        .build()
        .context("failed to build HTTP client for web search")?;

    let send = client
        .post(&config.generation_url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .header("X-DashScope-SSE", "enable")
        .header("Accept", "text/event-stream")
        .json(&body);

    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(anyhow!("已停止生成")),
        r = send.send() => r.context("web search HTTP request failed")?,
    };

    let status = resp.status();
    if !status.is_success() {
        let text = resp
            .text()
            .await
            .context("failed to read web search error body")?;
        warn!(
            "web_search generation error status={} url={} query={} response={}",
            status,
            config.generation_url,
            truncate_for_log(&req.query, 80),
            truncate_for_log(&text, 400)
        );
        return Err(anyhow!(
            "DashScope web search failed (HTTP {}): {}",
            status.as_u16(),
            truncate_for_log(&text, 400)
        ));
    }

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let result = if content_type.contains("text/event-stream") {
        let acc = read_dashscope_search_sse(resp.bytes_stream(), &cancel, ui.as_ref()).await?;
        acc.into_result(&req.query, &config.model, &req.search_strategy)
    } else {
        let text = resp
            .text()
            .await
            .context("failed to read web search response body")?;
        let trimmed = text.trim_start();
        if trimmed.starts_with("data:") || trimmed.starts_with("id:") || text.contains("\ndata:") {
            let acc = read_dashscope_search_sse_from_str(&text, ui.as_ref())?;
            acc.into_result(&req.query, &config.model, &req.search_strategy)
        } else {
            let parsed = parse_generation_json_fallback(&text).await?;
            let result =
                parse_search_response(&req.query, &config.model, &req.search_strategy, &parsed)?;
            if let Some(ref ui_ctx) = ui {
                if !result.sources.is_empty() {
                    ui_ctx.emit_sources_ready(&result.sources, result.search_count);
                }
                if !result.answer.is_empty() {
                    ui_ctx.emit_output_delta(&result.answer);
                }
            }
            result
        }
    };

    info!(
        "web_search generation ok model={} url={} query={} strategy={} sources={} search_count={} latency_ms={} request_id={:?}",
        config.model,
        config.generation_url,
        truncate_for_log(&req.query, 80),
        req.search_strategy,
        result.sources.len(),
        result.search_count,
        started.elapsed().as_millis(),
        result.request_id
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ModelSettings;

    #[test]
    fn resolve_tool_model_defaults_to_qwen3_max() {
        assert_eq!(resolve_tool_generation_model(""), "qwen3-max");
    }

    #[test]
    fn generation_url_uses_text_generation_for_qwen3_max() {
        let cfg = resolve_dashscope_generation_config(
            &ModelSettings {
                providers: vec![crate::models::ProviderConfig {
                    id: "qwen".into(),
                    name: "Qwen".into(),
                    base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
                    api_key: "sk".into(),
                    models: vec!["qwen3-max".into()],
                    reasoning_in_messages: None,
                    temperature: None,
                    top_p: None,
                    max_tokens: None,
                    context_budget_tokens: None,
                    model_configs: Default::default(),
                    enable_thinking: None,
                    thinking_budget: None,
                    reasoning_effort: None,
                    thinking_protocol: None,
                    thinking_intensity: None,
                    extra_body: None,
                    source: None,
                }],
                ..Default::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(cfg.model, "qwen3-max");
        assert!(cfg.generation_url.contains("text-generation"));
    }
}
