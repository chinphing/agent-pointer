//! DashScope native API client for web search (`enable_search` + `search_info`).

use crate::models::{ChatMessage, ModelSettings, Role};
use anyhow::{anyhow, Result};
use log::{info, warn};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::LazyLock;

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
    /// Offset applied to this call's citation indices (sum of prior search max indices in the same user turn).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation_base_index: Option<u32>,
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
        "enable_citation": true,
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
                } else if let Some(thoughts) =
                    msg.thoughts.as_deref().filter(|s| !s.trim().is_empty())
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

/// Markdown numbered list; index matches inline `[N]` / `[ref_N]` markers in `answer`.
pub fn format_sources_citation_markdown(sources: &[WebSearchSource]) -> String {
    if sources.is_empty() {
        return String::new();
    }
    let mut sorted: Vec<&WebSearchSource> = sources.iter().collect();
    sorted.sort_by_key(|s| s.index);
    let mut lines = vec!["Index map ([N] in answer → source):".to_string()];
    for s in sorted {
        let label = source_display_label(s);
        lines.push(format!("{}. [{}]({})", s.index, label, s.url.trim()));
    }
    lines.join("\n")
}

fn normalize_source_url_key(url: &str) -> String {
    url.trim().trim_end_matches('/').to_ascii_lowercase()
}

/// Dedupe by URL for display only (keeps first title; does not change `sources` indices).
fn dedupe_sources_for_display(sources: &[WebSearchSource]) -> Vec<WebSearchSource> {
    let mut seen = std::collections::HashSet::new();
    sources
        .iter()
        .filter(|s| {
            let key = normalize_source_url_key(&s.url);
            !key.is_empty() && seen.insert(key)
        })
        .cloned()
        .collect()
}

/// Single Sources block for user reply — numbered linked titles (`N` matches inline `[N]` in `answer`).
pub fn format_sources_for_reply(sources: &[WebSearchSource]) -> String {
    let mut deduped = dedupe_sources_for_display(sources);
    if deduped.is_empty() {
        return String::new();
    }
    deduped.sort_by_key(|s| s.index);
    let mut lines = vec!["## Sources".to_string(), String::new()];
    for s in deduped {
        let label = source_reply_label(&s);
        lines.push(format!("{}. [{label}]({})", s.index, s.url.trim()));
    }
    lines.join("\n")
}

/// Merge sources from multiple `web_search` calls: dedupe by URL, renumber 1..N for one Sources block.
pub fn format_merged_sources_for_reply(source_batches: &[&[WebSearchSource]]) -> String {
    let mut merged: Vec<WebSearchSource> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for batch in source_batches {
        let mut sorted: Vec<&WebSearchSource> = batch.iter().collect();
        sorted.sort_by_key(|s| s.index);
        for s in sorted {
            let key = normalize_source_url_key(&s.url);
            if key.is_empty() || !seen.insert(key) {
                continue;
            }
            merged.push((*s).clone());
        }
    }
    if merged.is_empty() {
        return String::new();
    }
    let mut lines = vec!["## Sources".to_string(), String::new()];
    for (i, s) in merged.iter().enumerate() {
        let n = (i + 1) as u32;
        let label = source_reply_label(s);
        lines.push(format!("{n}. [{label}]({})", s.url.trim()));
    }
    lines.join("\n")
}

/// Plain title list (one per line). For agent reference only — do not paste alongside `sourcesForReply`.
pub fn format_sources_title_list(sources: &[WebSearchSource]) -> String {
    if sources.is_empty() {
        return String::new();
    }
    let mut sorted: Vec<&WebSearchSource> = sources.iter().collect();
    sorted.sort_by_key(|s| s.index);
    sorted
        .iter()
        .map(|s| source_display_label(s))
        .collect::<Vec<_>>()
        .join("\n")
}

static REF_CITATION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[ref_(\d+)\]").expect("ref citation regex"));

fn linkify_bracket_index_citations(answer: &str, map: &HashMap<u32, &WebSearchSource>) -> String {
    let mut out = String::with_capacity(answer.len());
    let bytes = answer.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some((idx_str, consumed)) = parse_bracket_index_token(&answer[i..]) {
                let next = i + consumed;
                if next < bytes.len() && bytes[next] == b'(' {
                    out.push('[');
                    i += 1;
                    continue;
                }
                out.push_str(&citation_markdown_link(idx_str, map));
                i = next;
                continue;
            }
        }
        let ch = answer[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `[N]` where N is digits; not `[ref_N]` (handled separately).
fn parse_bracket_index_token(s: &str) -> Option<(&str, usize)> {
    let rest = s.strip_prefix('[')?;
    if rest.starts_with("ref_") {
        return None;
    }
    let digit_len = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digit_len == 0 {
        return None;
    }
    let after_digits = &rest[digit_len..];
    if !after_digits.starts_with(']') {
        return None;
    }
    Some((&rest[..digit_len], 1 + digit_len + 1))
}

/// Replace DashScope `[N]` / `[ref_N]` markers with Markdown `[title](url)` using `sources[].index`.
pub fn resolve_web_search_citations(answer: &str, sources: &[WebSearchSource]) -> String {
    if sources.is_empty() || !answer.contains('[') {
        return answer.to_string();
    }
    let map: HashMap<u32, &WebSearchSource> = sources.iter().map(|s| (s.index, s)).collect();
    let after_ref = REF_CITATION_RE.replace_all(answer, |caps: &regex::Captures| {
        citation_markdown_link(caps.get(1).map(|m| m.as_str()).unwrap_or(""), &map)
    });
    linkify_bracket_index_citations(&after_ref, &map)
}

fn source_display_label(src: &WebSearchSource) -> String {
    let title = src.title.trim();
    if !title.is_empty() {
        return title.replace('[', "\\[").replace(']', "\\]");
    }
    if let Some(site) = src.site_name.as_deref().filter(|s| !s.trim().is_empty()) {
        return site.trim().to_string();
    }
    format!("Source {}", src.index)
}

/// Label for Sources blocks — includes site name when available for trust/context.
fn source_reply_label(src: &WebSearchSource) -> String {
    let title = source_display_label(src);
    let Some(site) = src
        .site_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return title;
    };
    if title.eq_ignore_ascii_case(site) {
        return title;
    }
    format!("{site} · {title}")
}

fn citation_markdown_link(index_str: &str, map: &HashMap<u32, &WebSearchSource>) -> String {
    let Ok(idx) = index_str.parse::<u32>() else {
        return format!("[{index_str}]");
    };
    let Some(src) = map.get(&idx) else {
        return format!("[{idx}]");
    };
    let label = source_display_label(src);
    format!("[{label}]({})", src.url.trim())
}

/// Max `sources[].index` from prior successful `web_search` tool results since the last user message.
pub fn compute_citation_base_index(history: &[ChatMessage], exclude_message_id: &str) -> u32 {
    max_web_search_source_index(web_search_turn_slice(history, exclude_message_id))
}

fn web_search_turn_slice<'a>(
    history: &'a [ChatMessage],
    exclude_message_id: &str,
) -> &'a [ChatMessage] {
    let mut start = 0usize;
    for (i, m) in history.iter().enumerate().rev() {
        if m.id == exclude_message_id {
            continue;
        }
        if matches!(m.role, Role::User) {
            start = i + 1;
            break;
        }
    }
    &history[start..]
}

fn max_web_search_source_index(messages: &[ChatMessage]) -> u32 {
    let mut max_idx = 0u32;
    for m in messages {
        if !matches!(m.role, Role::Tool) {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&m.content) else {
            continue;
        };
        if !v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false) {
            continue;
        }
        if v.get("sources").and_then(|s| s.as_array()).is_none() {
            continue;
        }
        if let Some(sources) = v.get("sources").and_then(|s| s.as_array()) {
            for item in sources {
                if let Some(idx) = item.get("index").and_then(|x| x.as_u64()) {
                    max_idx = max_idx.max(idx as u32);
                }
            }
        }
    }
    max_idx
}

fn offset_bracket_index_markers(answer: &str, base: u32) -> String {
    if base == 0 {
        return answer.to_string();
    }
    let mut out = String::with_capacity(answer.len());
    let bytes = answer.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some((idx_str, consumed)) = parse_bracket_index_token(&answer[i..]) {
                let next = i + consumed;
                if next < bytes.len() && bytes[next] == b'(' {
                    out.push('[');
                    i += 1;
                    continue;
                }
                let Ok(idx) = idx_str.parse::<u32>() else {
                    out.push('[');
                    i += 1;
                    continue;
                };
                out.push_str(&format!("[{}]", idx + base));
                i = next;
                continue;
            }
        }
        let ch = answer[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Shift `[N]` / `[ref_N]` and `sources[].index` by `base_index` so multi-search citations stay unique.
pub fn apply_citation_base_index(mut result: WebSearchResult, base_index: u32) -> WebSearchResult {
    result.citation_base_index = Some(base_index);
    if base_index == 0 {
        return result;
    }
    for s in &mut result.sources {
        s.index = s.index.saturating_add(base_index);
    }
    let after_ref = REF_CITATION_RE.replace_all(&result.answer, |caps: &regex::Captures| {
        let raw = caps.get(1).map(|m| m.as_str()).unwrap_or("0");
        let Ok(idx) = raw.parse::<u32>() else {
            return format!("[ref_{raw}]");
        };
        format!("[ref_{}]", idx.saturating_add(base_index))
    });
    result.answer = offset_bracket_index_markers(&after_ref, base_index);
    result
}

/// Linkify inline citations in `answer` before returning tool JSON to the orchestrator.
pub fn finalize_web_search_result(mut result: WebSearchResult) -> WebSearchResult {
    result.answer = resolve_web_search_citations(&result.answer, &result.sources);
    result
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
        Ok(pretty) => {
            info!("web_search HTTP request (streaming={streaming})\nURL: {url}\nBody:\n{pretty}")
        }
        Err(e) => {
            warn!("web_search request body pretty-print failed: {e}");
            info!("web_search HTTP request (streaming={streaming})\nURL: {url}\nBody: {body}");
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
        let input_tokens = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let output_tokens = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let total_tokens = u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
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

    pub fn into_result(self, query: &str, model: &str, search_strategy: &str) -> WebSearchResult {
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
            citation_base_index: None,
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
        citation_base_index: None,
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
        return if t.is_empty() {
            String::new()
        } else {
            "…".into()
        };
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
        let r = finalize_web_search_result(
            parse_search_response("test query", "qwen-plus", "turbo", &body).unwrap(),
        );
        assert_eq!(r.answer, "Answer text [Example](https://example.com)");
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
                top_p: None,
                max_tokens: None,
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
                top_p: None,
                max_tokens: None,
                model_configs: Default::default(),
                enable_thinking: None,
                thinking_budget: None,
                reasoning_effort: None,
                thinking_protocol: None,
                thinking_intensity: None,
                extra_body: None,
                source: None,
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
                top_p: None,
                max_tokens: None,
                model_configs: Default::default(),
                enable_thinking: None,
                thinking_budget: None,
                reasoning_effort: None,
                thinking_protocol: None,
                thinking_intensity: None,
                extra_body: None,
                source: None,
            }],
            web_search_model: "qwen3.6-plus".into(),
            agent_default_models: [(
                "explore".into(),
                AgentModelRef {
                    provider_id: "qwen".into(),
                    model: "qwen3.6-plus".into(),
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let cfg = resolve_dashscope_search_config(&settings, Some("explore")).unwrap();
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
            tool_name: None,
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
            ui_bindings: None,
            context_state: None,
            attachments: None,
            anchor_message_id: None,
            trace_id: None,
            task_id: None,
            spawn_depth: None,
            tool_raw_output: None,
        };
        let history = vec![
            mk("u1", Role::User, "Compare Rust editions"),
            mk("a1", Role::Assistant, "I'll search the web."),
            mk("pending", Role::Assistant, ""),
        ];
        let msgs =
            history_to_dashscope_messages(&history, "pending", "Find 2024 edition release notes");
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
        assert_eq!(
            body["parameters"]["search_options"]["enable_citation"],
            json!(true)
        );
        assert!(!body["parameters"]
            .as_object()
            .unwrap()
            .contains_key("enable_thinking"));
    }

    #[test]
    fn apply_citation_base_index_offsets_markers_and_sources() {
        let raw = WebSearchResult {
            ok: true,
            query: "q".into(),
            answer: "See [1][2] and [ref_1].".into(),
            sources: vec![
                WebSearchSource {
                    index: 1,
                    title: "A".into(),
                    url: "https://a.example".into(),
                    site_name: None,
                },
                WebSearchSource {
                    index: 2,
                    title: "B".into(),
                    url: "https://b.example".into(),
                    site_name: None,
                },
            ],
            search_count: 1,
            usage: WebSearchUsage::default(),
            model: "qwen3-max".into(),
            search_strategy: "pro_max".into(),
            request_id: None,
            citation_base_index: None,
        };
        let shifted = apply_citation_base_index(raw, 7);
        assert_eq!(shifted.citation_base_index, Some(7));
        assert_eq!(shifted.sources[0].index, 8);
        assert_eq!(shifted.sources[1].index, 9);
        assert_eq!(shifted.answer, "See [8][9] and [ref_8].");
        let linked = finalize_web_search_result(shifted);
        assert!(linked.answer.contains("[A](https://a.example)"));
        assert!(linked.answer.contains("[B](https://b.example)"));
    }

    #[test]
    fn compute_citation_base_index_from_prior_tool_results() {
        use crate::models::ChatMessage;
        let prior = serde_json::json!({
            "ok": true,
            "sources": [{ "index": 7, "title": "X", "url": "https://x.example" }]
        });
        let history = vec![
            ChatMessage {
                id: "u1".into(),
                role: Role::User,
                content: "question".into(),
                status: "done".into(),
                created_at: 0,
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
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
                ui_bindings: None,
                context_state: None,
                attachments: None,
                anchor_message_id: None,
                trace_id: None,
                task_id: None,
                spawn_depth: None,
                tool_raw_output: None,
            },
            ChatMessage {
                id: "a1".into(),
                role: Role::Assistant,
                content: String::new(),
                status: "done".into(),
                created_at: 1,
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
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
                ui_bindings: None,
                context_state: None,
                attachments: None,
                anchor_message_id: None,
                trace_id: None,
                task_id: None,
                spawn_depth: None,
                tool_raw_output: None,
            },
            ChatMessage {
                id: "t1".into(),
                role: Role::Tool,
                content: prior.to_string(),
                status: "done".into(),
                created_at: 2,
                tool_calls: None,
                tool_call_id: Some("tc1".into()),
                tool_name: None,
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
                ui_bindings: None,
                context_state: None,
                attachments: None,
                anchor_message_id: None,
                trace_id: None,
                task_id: None,
                spawn_depth: None,
                tool_raw_output: None,
            },
        ];
        assert_eq!(compute_citation_base_index(&history, "a1"), 7);
    }

    #[test]
    fn format_merged_sources_for_reply_dedupes_and_renumbers() {
        let batch_a = [WebSearchSource {
            index: 2,
            title: "Report A".into(),
            url: "https://example.com/a".into(),
            site_name: None,
        }];
        let batch_b = [
            WebSearchSource {
                index: 1,
                title: "Report B".into(),
                url: "https://example.com/b".into(),
                site_name: None,
            },
            WebSearchSource {
                index: 3,
                title: "Dup A".into(),
                url: "https://example.com/a/".into(),
                site_name: None,
            },
        ];
        let merged = format_merged_sources_for_reply(&[&batch_a, &batch_b]);
        assert!(merged.contains("1. [Report A](https://example.com/a)"));
        assert!(merged.contains("2. [Report B](https://example.com/b)"));
        assert!(!merged.contains("Dup A"));
    }

    #[test]
    fn format_sources_for_reply_includes_site_name() {
        let reply = format_sources_for_reply(&[WebSearchSource {
            index: 1,
            title: "Gemini 3.5 Flash launch".into(),
            url: "https://example.com/a".into(),
            site_name: Some("Example News".into()),
        }]);
        assert!(
            reply.contains("1. [Example News · Gemini 3.5 Flash launch](https://example.com/a)")
        );
    }

    #[test]
    fn format_sources_for_reply_numbered_linked() {
        let reply = format_sources_for_reply(&[
            WebSearchSource {
                index: 6,
                title: "Gemini 3.5 Flash launch".into(),
                url: "https://example.com/a".into(),
                site_name: None,
            },
            WebSearchSource {
                index: 2,
                title: "Earlier source".into(),
                url: "https://example.com/b".into(),
                site_name: None,
            },
            WebSearchSource {
                index: 3,
                title: "Duplicate URL title".into(),
                url: "https://example.com/a/".into(),
                site_name: None,
            },
        ]);
        assert!(reply.contains("## Sources"));
        assert!(reply.contains("2. [Earlier source](https://example.com/b)"));
        assert!(reply.contains("6. [Gemini 3.5 Flash launch](https://example.com/a)"));
        assert!(!reply.contains("3. [Duplicate URL"));
        let pos2 = reply.find("2. [Earlier").unwrap();
        let pos6 = reply.find("6. [Gemini").unwrap();
        assert!(pos2 < pos6);
    }

    #[test]
    fn format_sources_citation_markdown_uses_title_and_url() {
        let md = format_sources_citation_markdown(&[WebSearchSource {
            index: 2,
            title: "2026 market outlook".into(),
            url: "https://example.com/report".into(),
            site_name: Some("Example".into()),
        }]);
        assert!(md.contains("2. [2026 market outlook](https://example.com/report)"));
        assert!(md.contains("Index map"));
    }

    #[test]
    fn resolve_citations_linkifies_index_and_ref_markers() {
        let sources = vec![
            WebSearchSource {
                index: 2,
                title: "Report A".into(),
                url: "https://a.example".into(),
                site_name: None,
            },
            WebSearchSource {
                index: 6,
                title: "Report B".into(),
                url: "https://b.example".into(),
                site_name: None,
            },
        ];
        let out = resolve_web_search_citations("Growth [2][6] and [ref_2].", &sources);
        assert!(out.contains("[Report A](https://a.example)"));
        assert!(out.contains("[Report B](https://b.example)"));
        assert!(!out.contains("[2]"));
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
