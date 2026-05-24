//! DashScope native API client for web search (`enable_search` + `search_info`).

use crate::models::{ChatMessage, ModelSettings, Role};
use anyhow::{anyhow, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const DEFAULT_WEB_SEARCH_TIMEOUT_SECS: u64 = 120;
/// Default `search_options.search_strategy` for generic tool (Generation API).
pub const DEFAULT_TOOL_WEB_SEARCH_STRATEGY: &str = "pro_max";
/// Alias kept for callers that import the tool default strategy name.
pub const DEFAULT_WEB_SEARCH_STRATEGY: &str = DEFAULT_TOOL_WEB_SEARCH_STRATEGY;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchSource {
    pub index: u32,
    pub title: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchResult {
    pub ok: bool,
    pub query: String,
    pub answer: String,
    pub sources: Vec<WebSearchSource>,
    pub search_count: u32,
    pub usage: WebSearchUsage,
    pub model: String,
    pub search_strategy: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebSearchMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct WebSearchRequest {
    pub query: String,
    pub search_strategy: String,
    pub forced_search: bool,
    pub enable_vertical_search: bool,
    /// DashScope `parameters.enable_thinking` (required for `agent_max` on some models).
    pub enable_thinking: bool,
    /// Conversation context forwarded to DashScope `input.messages` (final entry is `query`).
    pub messages: Vec<WebSearchMessage>,
}

pub struct DashScopeSearchConfig {
    pub api_key: String,
    pub generation_url: String,
    pub model: String,
}

/// Whether the model must use DashScope `multimodal-generation` (not `text-generation`).
pub fn dashscope_model_uses_multimodal_endpoint(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    if m.contains("-vl-") || m.contains("omni") {
        return true;
    }
    for stem in [
        "qwen3.5-plus",
        "qwen3.5-flash",
        "qwen3.6-plus",
        "qwen3.6-flash",
        "qwen3.7-max",
    ] {
        if m.starts_with(stem) {
            return true;
        }
    }
    false
}

/// Models whose web search is Responses-API-only on DashScope (not `Generation.enable_search`).
pub fn web_search_unsupported_on_generation_api(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    m.starts_with("qwen3.6-") || m.starts_with("qwen3.7-max")
}

/// Pick a model that works with our Generation + `enable_search` client.
pub fn resolve_web_search_api_model(configured: &str) -> String {
    let m = configured.trim();
    if m.is_empty() || web_search_unsupported_on_generation_api(m) {
        if !m.is_empty() {
            warn!(
                "web_search: model {m} is not supported on Generation API search; using {}",
                super::generation::DEFAULT_TOOL_WEB_SEARCH_MODEL
            );
        }
        return super::generation::DEFAULT_TOOL_WEB_SEARCH_MODEL.to_string();
    }
    m.to_string()
}

fn dashscope_api_service_path(model: &str) -> &'static str {
    if dashscope_model_uses_multimodal_endpoint(model) {
        "multimodal-generation/generation"
    } else {
        "text-generation/generation"
    }
}

/// Derive DashScope native generation URL from a compatible-mode base URL and model id.
pub fn dashscope_native_generation_url(compatible_base: &str, model: &str) -> String {
    let service = dashscope_api_service_path(model);
    let trimmed = compatible_base.trim().trim_end_matches('/');
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("/api/v1/services/aigc/") && lower.ends_with("/generation") {
        return trimmed.to_string();
    }
    for host in [
        "dashscope.aliyuncs.com",
        "dashscope-intl.aliyuncs.com",
        "dashscope-us.aliyuncs.com",
    ] {
        if lower.contains(host) {
            return format!("https://{host}/api/v1/services/aigc/{service}");
        }
    }
    if lower.ends_with("/compatible-mode/v1") {
        let origin = trimmed.trim_end_matches("/compatible-mode/v1");
        return format!("{origin}/api/v1/services/aigc/{service}");
    }
    format!("{trimmed}/api/v1/services/aigc/{service}")
}

pub fn resolve_dashscope_search_config(
    settings: &ModelSettings,
    agent_id: Option<&str>,
) -> Result<DashScopeSearchConfig> {
    super::generation::resolve_dashscope_generation_config(settings, agent_id)
}

/// Generation API body for generic tool calls (matches DashScope `enable_search` curl).
pub fn build_tool_generation_request_body(model: &str, req: &WebSearchRequest) -> Value {
    let mut search_options = json!({
        "search_strategy": req.search_strategy,
        "enable_source": true,
    });
    if req.forced_search {
        search_options["forced_search"] = json!(true);
    }
    if req.enable_vertical_search {
        search_options["enable_search_extension"] = json!(true);
    }

    let api_messages: Vec<Value> = if req.messages.is_empty() {
        vec![json!({ "role": "user", "content": req.query })]
    } else {
        req.messages
            .iter()
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect()
    };

    let mut parameters = json!({
        "enable_search": true,
        "search_options": search_options,
        "result_format": "message",
    });
    if req.enable_thinking {
        parameters["enable_thinking"] = json!(true);
    }

    json!({
        "model": model,
        "input": {
            "messages": api_messages,
        },
        "parameters": parameters,
    })
}

/// Tool streaming body: `incremental_output` + early `prepend_search_result` for sources.
pub fn build_tool_generation_stream_request_body(model: &str, req: &WebSearchRequest) -> Value {
    let mut body = build_tool_generation_request_body(model, req);
    if let Some(params) = body.get_mut("parameters").and_then(|p| p.as_object_mut()) {
        params.insert("incremental_output".into(), json!(true));
        if let Some(opts) = params
            .get_mut("search_options")
            .and_then(|o| o.as_object_mut())
        {
            opts.insert("prepend_search_result".into(), json!(true));
        }
    }
    body
}

/// Build DashScope `input.messages` from chat history plus the current search `query`.
///
/// Skips the in-flight assistant turn (`exclude_message_id`) and maps roles to DashScope format.
pub fn history_to_dashscope_messages(
    history: &[ChatMessage],
    exclude_message_id: &str,
    query: &str,
) -> Vec<WebSearchMessage> {
    let mut messages = Vec::new();
    for msg in history {
        if msg.id == exclude_message_id {
            continue;
        }
        match msg.role {
            Role::System => {
                let text = msg.content.trim();
                if !text.is_empty() {
                    messages.push(WebSearchMessage {
                        role: "system".into(),
                        content: text.to_string(),
                    });
                }
            }
            Role::User => {
                let text = msg.content.trim();
                if !text.is_empty() {
                    messages.push(WebSearchMessage {
                        role: "user".into(),
                        content: text.to_string(),
                    });
                }
            }
            Role::Assistant => {
                let mut parts = Vec::new();
                if !msg.content.trim().is_empty() {
                    parts.push(msg.content.trim().to_string());
                } else if let Some(thoughts) = msg.thoughts.as_deref().filter(|s| !s.trim().is_empty())
                {
                    parts.push(thoughts.trim().to_string());
                }
                if !parts.is_empty() {
                    messages.push(WebSearchMessage {
                        role: "assistant".into(),
                        content: parts.join("\n\n"),
                    });
                }
            }
            Role::Tool => {
                let text = msg.content.trim();
                if !text.is_empty() {
                    messages.push(WebSearchMessage {
                        role: "user".into(),
                        content: format!("[Tool result]\n{text}"),
                    });
                }
            }
        }
    }
    let q = query.trim();
    if !q.is_empty() {
        messages.push(WebSearchMessage {
            role: "user".into(),
            content: q.to_string(),
        });
    }
    messages
}

pub fn normalize_search_strategy(raw: &str) -> Result<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "pro_max" => Ok("pro_max"),
        "max" => Ok("max"),
        "agent_max" => Ok("agent_max"),
        "turbo" => Ok("turbo"),
        "agent" => Ok("agent"),
        other => Err(anyhow!(
            "invalid searchStrategy `{other}`; expected pro_max, max, agent_max, turbo, or agent"
        )),
    }
}

pub fn build_search_request_body(model: &str, req: &WebSearchRequest) -> Value {
    let mut search_options = json!({
        "enable_source": true,
        "enable_citation": true,
        "search_strategy": req.search_strategy,
    });
    if req.forced_search {
        search_options["forced_search"] = json!(true);
    }
    if req.enable_vertical_search {
        search_options["enable_search_extension"] = json!(true);
    }

    let api_messages: Vec<Value> = if req.messages.is_empty() {
        vec![json!({ "role": "user", "content": req.query })]
    } else {
        req.messages
            .iter()
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect()
    };

    json!({
        "model": model,
        "input": {
            "messages": api_messages,
        },
        "parameters": {
            "enable_search": true,
            "enable_thinking": req.enable_thinking,
            "search_options": search_options,
            "result_format": "message"
        }
    })
}

/// Streaming request body: adds `incremental_output` and `prepend_search_result`.
pub fn build_search_stream_request_body(model: &str, req: &WebSearchRequest) -> Value {
    let mut body = build_search_request_body(model, req);
    if let Some(params) = body.get_mut("parameters").and_then(|p| p.as_object_mut()) {
        params.insert("incremental_output".into(), json!(true));
        if let Some(opts) = params
            .get_mut("search_options")
            .and_then(|o| o.as_object_mut())
        {
            opts.insert("prepend_search_result".into(), json!(true));
        }
    }
    body
}

pub(crate) fn log_web_search_http_request(url: &str, body: &Value, streaming: bool) {
    match serde_json::to_string_pretty(body) {
        Ok(pretty) => info!(
            "web_search HTTP request (streaming={streaming})\nURL: {url}\nBody:\n{pretty}"
        ),
        Err(e) => {
            warn!("web_search request body pretty-print failed: {e}");
            info!(
                "web_search HTTP request (streaming={streaming})\nURL: {url}\nBody: {body}"
            );
        }
    }
}

/// One parsed DashScope search SSE JSON payload.
#[derive(Debug, Clone, Default)]
pub struct SearchSseChunk {
    pub content_delta: String,
    pub finish_reason: Option<String>,
    pub sources: Vec<WebSearchSource>,
    pub usage: Option<WebSearchUsage>,
    pub search_count: Option<u32>,
    pub request_id: Option<String>,
}

impl SearchSseChunk {
    pub fn from_json(body: &Value) -> Self {
        parse_search_sse_chunk(body)
    }
}

/// Parse a single DashScope search SSE `data:` JSON object.
pub fn parse_search_sse_chunk(body: &Value) -> SearchSseChunk {
    let output = body.get("output");
    let content_delta = output
        .and_then(|o| o.get("choices"))
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or_default()
        .to_string();
    let finish_reason = output
        .and_then(|o| o.get("choices"))
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let sources = output
        .and_then(|o| o.get("search_info"))
        .and_then(|s| s.get("search_results"))
        .and_then(|r| r.as_array())
        .map(|items| parse_sources(items))
        .unwrap_or_default();

    let usage_obj = body.get("usage");
    let usage = usage_obj.map(|u| {
        let input_tokens = u
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let output_tokens = u
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let total_tokens = u
            .get("total_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        WebSearchUsage {
            input_tokens,
            output_tokens,
            total_tokens,
        }
    });
    let search_count = usage_obj
        .and_then(|u| u.get("plugins"))
        .and_then(|p| p.get("search"))
        .and_then(|s| s.get("count"))
        .and_then(|c| c.as_u64())
        .map(|c| c as u32);
    let request_id = body
        .get("request_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    SearchSseChunk {
        content_delta,
        finish_reason,
        sources,
        usage,
        search_count,
        request_id,
    }
}

/// Aggregates incremental search SSE chunks into a final [`WebSearchResult`].
#[derive(Debug, Clone, Default)]
pub struct SearchSseAccumulator {
    pub answer: String,
    pub sources: Vec<WebSearchSource>,
    pub usage: WebSearchUsage,
    pub search_count: u32,
    pub request_id: Option<String>,
}

impl SearchSseAccumulator {
    pub fn apply_chunk(&mut self, chunk: &SearchSseChunk) {
        if !chunk.content_delta.is_empty() {
            self.answer.push_str(&chunk.content_delta);
        }
        if !chunk.sources.is_empty() && self.sources.is_empty() {
            self.sources = chunk.sources.clone();
        }
        if let Some(u) = &chunk.usage {
            if u.total_tokens > 0 {
                self.usage = u.clone();
            }
        }
        if let Some(c) = chunk.search_count {
            self.search_count = c;
        }
        if chunk.request_id.is_some() {
            self.request_id = chunk.request_id.clone();
        }
    }

    pub fn into_result(
        self,
        query: &str,
        model: &str,
        search_strategy: &str,
    ) -> WebSearchResult {
        WebSearchResult {
            ok: true,
            query: query.to_string(),
            answer: self.answer,
            sources: self.sources,
            search_count: self.search_count,
            usage: self.usage,
            model: model.to_string(),
            search_strategy: search_strategy.to_string(),
            request_id: self.request_id,
        }
    }
}

pub fn parse_search_response(
    query: &str,
    model: &str,
    search_strategy: &str,
    body: &Value,
) -> Result<WebSearchResult> {
    let output = body
        .get("output")
        .ok_or_else(|| anyhow!("DashScope response missing output"))?;

    let answer = output
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or_default()
        .to_string();

    let sources = output
        .get("search_info")
        .and_then(|s| s.get("search_results"))
        .and_then(|r| r.as_array())
        .map(|items| parse_sources(items))
        .unwrap_or_default();

    let usage_obj = body.get("usage");
    let input_tokens = usage_obj
        .and_then(|u| u.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let output_tokens = usage_obj
        .and_then(|u| u.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let total_tokens = usage_obj
        .and_then(|u| u.get("total_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let search_count = usage_obj
        .and_then(|u| u.get("plugins"))
        .and_then(|p| p.get("search"))
        .and_then(|s| s.get("count"))
        .and_then(|c| c.as_u64())
        .unwrap_or(0) as u32;

    let request_id = body
        .get("request_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    Ok(WebSearchResult {
        ok: true,
        query: query.to_string(),
        answer,
        sources,
        search_count,
        usage: WebSearchUsage {
            input_tokens,
            output_tokens,
            total_tokens,
        },
        model: model.to_string(),
        search_strategy: search_strategy.to_string(),
        request_id,
    })
}

fn parse_sources(items: &[Value]) -> Vec<WebSearchSource> {
    items
        .iter()
        .filter_map(|item| {
            let index = item.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let url = item
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if url.is_empty() {
                return None;
            }
            let site_name = item
                .get("site_name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            Some(WebSearchSource {
                index,
                title,
                url,
                site_name,
            })
        })
        .collect()
}

/// Generic tool: Generation API blocking search.
pub async fn execute_web_search_stream(
    settings: &ModelSettings,
    agent_id: Option<&str>,
    req: WebSearchRequest,
    cancel: tokio_util::sync::CancellationToken,
    ui: Option<super::stream_ui::WebSearchStreamUi>,
) -> Result<WebSearchResult> {
    super::generation::execute_generation_web_search(settings, agent_id, req, cancel, ui).await
}

pub async fn execute_web_search(
    settings: &ModelSettings,
    agent_id: Option<&str>,
    req: WebSearchRequest,
) -> Result<WebSearchResult> {
    super::generation::execute_generation_web_search(
        settings,
        agent_id,
        req,
        tokio_util::sync::CancellationToken::new(),
        None,
    )
    .await
}

pub(crate) fn truncate_for_log(s: &str, max_bytes: usize) -> String {
    let t = s.trim();
    // UTF-16 code units are fixed 2 bytes; truncate on unit boundaries (never split CJK/surrogates).
    let max_units = max_bytes / 2;
    if max_units <= 1 {
        return if t.is_empty() { String::new() } else { "…".into() };
    }
    let units: Vec<u16> = t.encode_utf16().collect();
    if units.len() <= max_units {
        return t.to_string();
    }
    let mut cut = max_units.saturating_sub(1);
    while cut > 0 && units[cut - 1] >= 0xD800 && units[cut - 1] <= 0xDBFF {
        cut -= 1;
    }
    if cut == 0 {
        return "…".to_string();
    }
    let prefix = String::from_utf16(&units[..cut]).unwrap_or_default();
    format!("{prefix}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ModelSettings, ProviderConfig};
    use serde_json::json;

    #[test]
    fn truncate_for_log_does_not_split_utf16_code_unit() {
        let query = "查找阿里云通义千问（Qwen）系列在2024年至2025年期间发布的最新大语言模型。重点关注：1. Qwen2.5系列的发布时间和主要改进；2. Qwen-Max、Qwen-Plus等商业模型的更新；3. 开源模型的版本演进。请提";
        let out = truncate_for_log(query, 80);
        assert!(out.ends_with('…'));
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
        assert!(out.encode_utf16().count() <= 80 / 2 + 1);
    }

    #[test]
    fn native_url_text_model_uses_text_generation() {
        assert_eq!(
            dashscope_native_generation_url(
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
                "qwen-plus",
            ),
            "https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation"
        );
    }

    #[test]
    fn native_url_multimodal_model_uses_multimodal_generation() {
        assert_eq!(
            dashscope_native_generation_url(
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
                "qwen3.5-plus",
            ),
            "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation"
        );
        assert_eq!(
            dashscope_native_generation_url(
                "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
                "qwen3.5-plus",
            ),
            "https://dashscope-intl.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation"
        );
    }

    #[test]
    fn resolve_api_model_falls_back_from_qwen36_plus() {
        assert_eq!(resolve_web_search_api_model("qwen3.6-plus"), "qwen3-max");
        assert_eq!(resolve_web_search_api_model("qwen-plus"), "qwen-plus");
    }

    #[test]
    fn parse_fixture_response() {
        let body = json!({
            "output": {
                "choices": [{
                    "message": { "content": "Answer text [ref_1]" }
                }],
                "search_info": {
                    "search_results": [{
                        "index": 1,
                        "title": "Example",
                        "url": "https://example.com",
                        "site_name": "Example"
                    }]
                }
            },
            "usage": {
                "input_tokens": 100,
                "output_tokens": 20,
                "total_tokens": 120,
                "plugins": { "search": { "count": 1 } }
            },
            "request_id": "req-1"
        });
        let r = parse_search_response("test query", "qwen-plus", "turbo", &body).unwrap();
        assert_eq!(r.answer, "Answer text [ref_1]");
        assert_eq!(r.sources.len(), 1);
        assert_eq!(r.search_count, 1);
        assert_eq!(r.request_id.as_deref(), Some("req-1"));
    }

    #[test]
    fn resolve_config_requires_key() {
        let settings = ModelSettings {
            providers: vec![ProviderConfig {
                id: "qwen".into(),
                name: "Qwen".into(),
                base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
                api_key: String::new(),
                models: vec!["qwen-plus".into()],
                reasoning_in_messages: None,
                temperature: None,
                max_tokens: None,
                model_configs: Default::default(),
                enable_thinking: None,
                thinking_budget: None,
                reasoning_effort: None,
            }],
            ..Default::default()
        };
        assert!(resolve_dashscope_search_config(&settings, None).is_err());
    }

    #[test]
    fn resolve_config_uses_qwen_provider_key_not_active_provider() {
        let settings = ModelSettings {
            providers: vec![ProviderConfig {
                id: "qwen".into(),
                name: "Qwen".into(),
                base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
                api_key: "sk-qwen".into(),
                models: vec!["qwen-plus".into()],
                reasoning_in_messages: None,
                temperature: None,
                max_tokens: None,
                model_configs: Default::default(),
                enable_thinking: None,
                thinking_budget: None,
                reasoning_effort: None,
            }],
            active_provider_id: "deepseek".into(),
            ..Default::default()
        };
        let cfg = resolve_dashscope_search_config(&settings, None).unwrap();
        assert_eq!(cfg.api_key, "sk-qwen");
    }

    #[test]
    fn resolve_config_falls_back_unsupported_web_search_model() {
        use crate::models::AgentModelRef;
        let settings = ModelSettings {
            providers: vec![ProviderConfig {
                id: "qwen".into(),
                name: "Qwen".into(),
                base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
                api_key: "sk-qwen".into(),
                models: vec!["qwen-plus".into()],
                reasoning_in_messages: None,
                temperature: None,
                max_tokens: None,
                model_configs: Default::default(),
                enable_thinking: None,
                thinking_budget: None,
                reasoning_effort: None,
            }],
            web_search_model: "qwen3.6-plus".into(),
            agent_default_models: [(
                "research".into(),
                AgentModelRef {
                    provider_id: "qwen".into(),
                    model: "qwen3.6-plus".into(),
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let cfg = resolve_dashscope_search_config(&settings, Some("research")).unwrap();
        assert_eq!(cfg.model, "qwen3-max");
        assert!(cfg.generation_url.contains("text-generation"));
    }

    #[test]
    fn history_to_messages_includes_roles_and_query() {
        use crate::models::{ChatMessage, Role};
        let mk = |id: &str, role: Role, content: &str| ChatMessage {
            id: id.into(),
            role,
            content: content.into(),
            status: "done".into(),
            created_at: 0,
            tool_calls: None,
            tool_call_id: None,
            error_message: None,
            reasoning: None,
            thoughts: None,
            headline: None,
            raw_content: None,
            agent_id: None,
            agent_instance_id: None,
            agent_name: None,
            agent_trace: None,
            images_base64: None,
            image_slot_labels: None,
            computer_round_screen_rel_path: None,
        };
        let history = vec![
            mk("u1", Role::User, "Compare Rust editions"),
            mk("a1", Role::Assistant, "I'll search the web."),
            mk("pending", Role::Assistant, ""),
        ];
        let msgs = history_to_dashscope_messages(&history, "pending", "Find 2024 edition release notes");
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[2].role, "user");
        assert!(msgs[2].content.contains("2024 edition"));
    }

    #[test]
    fn build_request_includes_search_options() {
    let req = WebSearchRequest {
        query: "weather".into(),
        search_strategy: "max".into(),
        forced_search: true,
        enable_vertical_search: true,
        enable_thinking: true,
        messages: vec![WebSearchMessage {
                role: "user".into(),
                content: "weather".into(),
            }],
        };
        let body = build_search_request_body("qwen-plus", &req);
        assert_eq!(body["parameters"]["enable_search"], json!(true));
        assert_eq!(
            body["parameters"]["search_options"]["search_strategy"],
            json!("max")
        );
        assert_eq!(
            body["parameters"]["search_options"]["forced_search"],
            json!(true)
        );
        assert_eq!(
            body["parameters"]["search_options"]["enable_search_extension"],
            json!(true)
        );
        assert_eq!(body["parameters"]["enable_thinking"], json!(true));
        assert_eq!(
            body["parameters"]["search_options"]["search_strategy"],
            json!("max")
        );
    }

    #[test]
    fn build_request_defaults_pro_max_without_thinking() {
        let req = WebSearchRequest {
            query: "weather".into(),
            search_strategy: DEFAULT_TOOL_WEB_SEARCH_STRATEGY.into(),
            forced_search: false,
            enable_vertical_search: false,
            enable_thinking: false,
            messages: vec![WebSearchMessage {
                role: "user".into(),
                content: "weather".into(),
            }],
        };
        let body = build_tool_generation_request_body("qwen3-max", &req);
        assert_eq!(body["model"], json!("qwen3-max"));
        assert_eq!(body["parameters"]["enable_search"], json!(true));
        assert_eq!(
            body["parameters"]["search_options"]["search_strategy"],
            json!("pro_max")
        );
        assert_eq!(
            body["parameters"]["search_options"]["enable_source"],
            json!(true)
        );
        assert!(!body["parameters"].as_object().unwrap().contains_key("enable_thinking"));
    }

    #[test]
    fn build_request_defaults_max_without_thinking() {
        let req = WebSearchRequest {
            query: "weather".into(),
            search_strategy: "max".into(),
            forced_search: false,
            enable_vertical_search: false,
            enable_thinking: false,
            messages: vec![WebSearchMessage {
                role: "user".into(),
                content: "weather".into(),
            }],
        };
        let body = build_search_request_body("qwen3-max", &req);
        assert_eq!(
            body["parameters"]["search_options"]["search_strategy"],
            json!("max")
        );
        assert_eq!(body["parameters"]["enable_thinking"], json!(false));
    }

    #[test]
    fn build_tool_stream_request_has_incremental_output() {
        let req = WebSearchRequest {
            query: "weather".into(),
            search_strategy: "pro_max".into(),
            forced_search: false,
            enable_vertical_search: false,
            enable_thinking: false,
            messages: vec![WebSearchMessage {
                role: "user".into(),
                content: "weather".into(),
            }],
        };
        let body = build_tool_generation_stream_request_body("qwen3-max", &req);
        assert_eq!(body["parameters"]["incremental_output"], json!(true));
        assert_eq!(
            body["parameters"]["search_options"]["prepend_search_result"],
            json!(true)
        );
        assert_eq!(
            body["parameters"]["search_options"]["search_strategy"],
            json!("pro_max")
        );
    }

    #[test]
    fn build_stream_request_has_incremental_output() {
        let req = WebSearchRequest {
            query: "weather".into(),
            search_strategy: "turbo".into(),
            forced_search: false,
            enable_vertical_search: false,
            enable_thinking: true,
            messages: vec![WebSearchMessage {
                role: "user".into(),
                content: "weather".into(),
            }],
        };
        let body = build_search_stream_request_body("qwen-plus", &req);
        assert_eq!(body["parameters"]["incremental_output"], json!(true));
        assert_eq!(
            body["parameters"]["search_options"]["prepend_search_result"],
            json!(true)
        );
    }

    #[test]
    fn parse_sse_chunks_aggregate_answer_and_sources() {
        let chunk1 = json!({
            "output": {
                "choices": [{ "message": { "content": "" }, "finish_reason": "null" }],
                "search_info": {
                    "search_results": [{
                        "index": 1,
                        "title": "Example",
                        "url": "https://example.com"
                    }]
                }
            }
        });
        let chunk2 = json!({
            "output": {
                "choices": [{ "message": { "content": "Hello" }, "finish_reason": "null" }]
            }
        });
        let chunk3 = json!({
            "output": {
                "choices": [{ "message": { "content": " world" }, "finish_reason": "stop" }]
            },
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5,
                "total_tokens": 15,
                "plugins": { "search": { "count": 1 } }
            },
            "request_id": "req-sse"
        });
        let mut acc = SearchSseAccumulator::default();
        acc.apply_chunk(&parse_search_sse_chunk(&chunk1));
        acc.apply_chunk(&parse_search_sse_chunk(&chunk2));
        acc.apply_chunk(&parse_search_sse_chunk(&chunk3));
        let r = acc.into_result("q", "qwen-plus", "turbo");
        assert_eq!(r.answer, "Hello world");
        assert_eq!(r.sources.len(), 1);
        assert_eq!(r.usage.total_tokens, 15);
        assert_eq!(r.request_id.as_deref(), Some("req-sse"));
    }
}
