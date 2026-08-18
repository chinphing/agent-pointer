//! DashScope **Responses API** (`/compatible-mode/v1/responses`) for agentic web search.

use crate::models::{
    effective_web_search_model, find_dashscope_provider, ModelSettings, DEFAULT_WEB_SEARCH_MODEL,
};
use anyhow::{anyhow, Context, Result};
use log::{info, warn};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use super::client::{
    log_web_search_http_request, truncate_for_log, DashScopeSearchConfig, WebSearchRequest,
    WebSearchResult, WebSearchSource, WebSearchUsage, DEFAULT_WEB_SEARCH_TIMEOUT_SECS,
};
use super::stream_ui::WebSearchStreamUi;

/// Derive Responses API URL from Qwen compatible-mode base URL.
pub fn dashscope_responses_url(compatible_base: &str) -> String {
    let trimmed = compatible_base.trim().trim_end_matches('/');
    let lower = trimmed.to_ascii_lowercase();
    if lower.ends_with("/responses") {
        return trimmed.to_string();
    }
    if lower.ends_with("/compatible-mode/v1") {
        return format!("{trimmed}/responses");
    }
    for host in [
        "dashscope.aliyuncs.com",
        "dashscope-intl.aliyuncs.com",
        "dashscope-us.aliyuncs.com",
    ] {
        if lower.contains(host) {
            return format!("https://{host}/compatible-mode/v1/responses");
        }
    }
    format!("{trimmed}/compatible-mode/v1/responses")
}

pub fn resolve_responses_api_model(configured: &str) -> String {
    let m = configured.trim();
    if m.is_empty() {
        DEFAULT_WEB_SEARCH_MODEL.to_string()
    } else {
        m.to_string()
    }
}

pub fn resolve_dashscope_responses_config(
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
    let model = resolve_responses_api_model(&configured);

    Ok(DashScopeSearchConfig {
        api_key: api_key.to_string(),
        generation_url: dashscope_responses_url(&provider.base_url),
        model,
    })
}

/// Flatten `messages` (research sub-agent) or use `query` only (tool mode).
pub fn build_responses_input(req: &WebSearchRequest) -> String {
    if req.messages.is_empty() {
        return req.query.trim().to_string();
    }
    if req.messages.len() == 1 {
        return req.messages[0].content.trim().to_string();
    }
    let mut parts = Vec::new();
    for m in &req.messages {
        let role = m.role.trim();
        let content = m.content.trim();
        if content.is_empty() {
            continue;
        }
        parts.push(format!("[{role}]\n{content}"));
    }
    if parts.is_empty() {
        req.query.trim().to_string()
    } else {
        parts.join("\n\n")
    }
}

pub fn build_responses_request_body(model: &str, req: &WebSearchRequest) -> Value {
    json!({
        "model": model,
        "input": build_responses_input(req),
        "tools": [
            { "type": "web_search" },
            { "type": "web_extractor" },
            { "type": "code_interpreter" }
        ],
        "enable_thinking": req.enable_thinking,
    })
}

pub fn parse_responses_response(
    query: &str,
    model: &str,
    search_strategy: &str,
    body: &Value,
) -> Result<WebSearchResult> {
    let answer = extract_final_message_text(body).unwrap_or_default();
    if answer.trim().is_empty() {
        warn!("web_search responses: empty assistant message in output");
    }

    let sources = extract_web_search_sources(body);
    let usage = parse_responses_usage(body);
    let search_count = parse_responses_search_count(body, &sources);
    let request_id = body.get("id").and_then(|v| v.as_str()).map(str::to_string);

    Ok(WebSearchResult {
        ok: true,
        query: query.to_string(),
        answer,
        sources,
        search_count,
        usage,
        model: model.to_string(),
        search_strategy: search_strategy.to_string(),
        request_id,
        citation_base_index: None,
    })
}

fn extract_final_message_text(body: &Value) -> Option<String> {
    let output = body.get("output")?.as_array()?;
    for item in output.iter().rev() {
        if item.get("type").and_then(|v| v.as_str()) != Some("message") {
            continue;
        }
        let text = item
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|block| block.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or_default();
        if !text.trim().is_empty() {
            return Some(text.to_string());
        }
    }
    None
}

fn extract_web_search_sources(body: &Value) -> Vec<WebSearchSource> {
    let Some(output) = body.get("output").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut sources = Vec::new();
    let mut index = 1u32;
    for item in output {
        if item.get("type").and_then(|v| v.as_str()) != Some("web_search_call") {
            continue;
        }
        let Some(url_items) = item.pointer("/action/sources").and_then(|v| v.as_array()) else {
            continue;
        };
        for src in url_items {
            let url = src
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim()
                .to_string();
            if url.is_empty() || !seen.insert(url.clone()) {
                continue;
            }
            let title = src
                .get("title")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| title_from_url(&url));
            sources.push(WebSearchSource {
                index,
                title,
                url,
                site_name: None,
            });
            index += 1;
        }
    }
    sources
}

fn title_from_url(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

fn parse_responses_usage(body: &Value) -> WebSearchUsage {
    let usage = body.get("usage");
    WebSearchUsage {
        input_tokens: usage
            .and_then(|u| u.get("input_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        output_tokens: usage
            .and_then(|u| u.get("output_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        total_tokens: usage
            .and_then(|u| u.get("total_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
    }
}

fn parse_responses_search_count(body: &Value, sources: &[WebSearchSource]) -> u32 {
    body.pointer("/usage/x_tools/web_search/count")
        .or_else(|| body.pointer("/usage/plugins/web_search/count"))
        .and_then(|v| v.as_u64())
        .map(|c| c as u32)
        .unwrap_or_else(|| {
            body.get("output")
                .and_then(|o| o.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter(|i| {
                            i.get("type").and_then(|t| t.as_str()) == Some("web_search_call")
                        })
                        .count() as u32
                })
                .unwrap_or(0)
                .max(if sources.is_empty() { 0 } else { 1 })
        })
}

pub async fn execute_responses_web_search(
    settings: &ModelSettings,
    agent_id: Option<&str>,
    req: WebSearchRequest,
    cancel: CancellationToken,
    ui: Option<WebSearchStreamUi>,
) -> Result<WebSearchResult> {
    let config = resolve_dashscope_responses_config(settings, agent_id)?;
    let body = build_responses_request_body(&config.model, &req);
    log_web_search_http_request(&config.generation_url, &body, false);
    let started = Instant::now();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(DEFAULT_WEB_SEARCH_TIMEOUT_SECS))
        .build()
        .context("failed to build HTTP client for web search")?;

    let send = client
        .post(&config.generation_url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .json(&body);

    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(anyhow!("已停止生成")),
        r = send.send() => r.context("web search HTTP request failed")?,
    };

    let status = resp.status();
    let text = resp
        .text()
        .await
        .context("failed to read web search response body")?;
    if !status.is_success() {
        warn!(
            "web_search DashScope error status={} url={} body_len={} query={} response={}",
            status,
            config.generation_url,
            text.len(),
            truncate_for_log(&req.query, 80),
            truncate_for_log(&text, 400)
        );
        return Err(anyhow!(
            "DashScope web search failed (HTTP {}): {}",
            status.as_u16(),
            truncate_for_log(&text, 400)
        ));
    }

    let parsed: Value =
        serde_json::from_str(&text).context("DashScope web search returned invalid JSON")?;
    let result =
        parse_responses_response(&req.query, &config.model, &req.search_strategy, &parsed)?;

    if let Some(ref ui_ctx) = ui {
        if !result.sources.is_empty() {
            ui_ctx.emit_sources_ready(&result.sources, result.search_count);
        }
        if !result.answer.is_empty() {
            ui_ctx.emit_output_delta(&result.answer);
        }
    }

    info!(
        "web_search responses ok model={} url={} query={} sources={} search_count={} latency_ms={} request_id={:?}",
        config.model,
        config.generation_url,
        truncate_for_log(&req.query, 80),
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
    use serde_json::json;

    #[test]
    fn responses_url_from_compatible_base() {
        assert_eq!(
            dashscope_responses_url("https://dashscope.aliyuncs.com/compatible-mode/v1"),
            "https://dashscope.aliyuncs.com/compatible-mode/v1/responses"
        );
    }

    #[test]
    fn parse_fixture_extracts_answer_and_sources() {
        let body = json!({
            "id": "resp_test",
            "output": [
                {
                    "type": "web_search_call",
                    "action": {
                        "sources": [
                            { "type": "url", "url": "https://example.com/a" },
                            { "type": "url", "url": "https://example.com/b" }
                        ]
                    }
                },
                {
                    "type": "message",
                    "content": [{ "type": "output_text", "text": "Answer body" }]
                }
            ],
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "total_tokens": 150,
                "x_tools": { "web_search": { "count": 1 } }
            }
        });
        let r = parse_responses_response("q", "qwen3-max-2026-01-23", "responses", &body).unwrap();
        assert_eq!(r.answer, "Answer body");
        assert_eq!(r.sources.len(), 2);
        assert_eq!(r.search_count, 1);
        assert_eq!(r.usage.total_tokens, 150);
    }

    #[test]
    fn build_body_matches_agent_tools() {
        let req = WebSearchRequest {
            query: "腾讯最新股价".into(),
            search_strategy: "responses".into(),
            forced_search: false,
            enable_vertical_search: false,
            enable_thinking: true,
            messages: vec![],
        };
        let body = build_responses_request_body("qwen3-max-2026-01-23", &req);
        assert_eq!(body["input"], json!("腾讯最新股价"));
        assert_eq!(body["enable_thinking"], json!(true));
        assert_eq!(body["tools"].as_array().unwrap().len(), 3);
    }
}
