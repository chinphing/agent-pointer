//! DeepSeek Responses API web search (`tools: [{type: web_search}]`).
//!
//! Official behavior: `web_search` runs server-side on `deepseek-v4-pro` only.
//! `deepseek-flash` ignores the tool, so this path always calls `deepseek-v4-pro`
//! unless the configured id is already a `deepseek-v4-pro` snapshot.

use crate::models::{
    provider_uses_dashscope_compatible_api, provider_uses_deepseek_api, ModelSettings,
    ProviderConfig, WEB_SEARCH_SCENE_ID,
};
use anyhow::{anyhow, Context, Result};
use log::{info, warn};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use super::client::{
    log_web_search_http_request, truncate_for_log, WebSearchRequest, WebSearchResult,
    WebSearchSource, WebSearchUsage, DEFAULT_WEB_SEARCH_TIMEOUT_SECS,
};
use super::stream_ui::WebSearchStreamUi;

/// Model that actually runs DeepSeek server-side web search.
pub const DEEPSEEK_WEB_SEARCH_MODEL: &str = "deepseek-v4-pro";

const SEARCH_INSTRUCTIONS: &str = "\
Search the public web with the web_search tool before answering. \
Answer from the search results and keep the cited pages.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WebSearchRoute {
    DashScope,
    DeepSeek {
        provider_id: String,
        requested_model: String,
    },
}

/// `deepseek` preset id, or any provider whose base URL is the official host.
pub fn provider_is_deepseek_search(provider: &ProviderConfig) -> bool {
    provider.id.eq_ignore_ascii_case("deepseek") || provider_uses_deepseek_api(provider)
}

/// Responses endpoint. Strips a trailing `/v1` so `https://api.deepseek.com/v1`
/// becomes `https://api.deepseek.com/responses`.
pub fn deepseek_responses_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return "https://api.deepseek.com/responses".to_string();
    }
    let lower = trimmed.to_ascii_lowercase();
    let origin = if let Some(stripped) = strip_suffix_ci(trimmed, &lower, "/compatible-mode/v1") {
        stripped
    } else if let Some(stripped) = strip_suffix_ci(trimmed, &lower, "/v1") {
        stripped
    } else {
        trimmed
    };
    let origin = origin.trim_end_matches('/');
    if origin.is_empty() {
        return "https://api.deepseek.com/responses".to_string();
    }
    format!("{origin}/responses")
}

fn strip_suffix_ci<'a>(original: &'a str, lower: &str, suffix: &str) -> Option<&'a str> {
    if lower.ends_with(suffix) {
        Some(&original[..original.len() - suffix.len()])
    } else {
        None
    }
}

/// Returns `(model_on_the_wire, substituted)`.
/// Empty input uses `deepseek-v4-pro` without counting as a substitution.
pub fn resolve_deepseek_web_search_model(requested: &str) -> (String, bool) {
    let trimmed = requested.trim();
    let lower = trimmed.to_ascii_lowercase();
    let pro = DEEPSEEK_WEB_SEARCH_MODEL;
    let pro_snapshot = lower
        .strip_prefix(pro)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'));
    if pro_snapshot {
        return (trimmed.to_string(), false);
    }
    (pro.to_string(), !trimmed.is_empty())
}

pub fn build_deepseek_web_search_body(model: &str, req: &WebSearchRequest) -> Value {
    let input = if req.messages.len() <= 1 {
        json!(req.query)
    } else {
        Value::Array(
            req.messages
                .iter()
                .map(|m| json!({ "role": m.role, "content": m.content }))
                .collect(),
        )
    };
    let mut body = json!({
        "model": model,
        "input": input,
        "instructions": SEARCH_INSTRUCTIONS,
        "tools": [{ "type": "web_search" }],
        "stream": true,
    });
    if req.enable_thinking {
        body["reasoning"] = json!({ "effort": "high" });
    }
    body
}

#[derive(Debug, Clone, Default)]
struct DeepSeekAccum {
    answer: String,
    saw_text: bool,
    sources: Vec<WebSearchSource>,
    usage: WebSearchUsage,
    usage_search_count: Option<u32>,
    request_id: Option<String>,
    call_ids: HashSet<String>,
    anonymous_calls: u32,
}

impl DeepSeekAccum {
    fn into_result(self, query: &str, model: &str, search_strategy: &str) -> WebSearchResult {
        let calls = self.call_ids.len() as u32 + self.anonymous_calls;
        let search_count = self
            .usage_search_count
            .filter(|n| *n > 0)
            .unwrap_or(if calls > 0 {
                calls
            } else if self.sources.is_empty() {
                0
            } else {
                1
            });
        WebSearchResult {
            ok: true,
            query: query.to_string(),
            answer: self.answer,
            sources: self.sources,
            search_count,
            usage: self.usage,
            model: model.to_string(),
            search_strategy: search_strategy.to_string(),
            request_id: self.request_id,
            citation_base_index: None,
        }
    }
}

fn normalize_url_key(url: &str) -> String {
    url.trim().trim_end_matches('/').to_ascii_lowercase()
}

fn title_or_host(title: &str, url: &str) -> String {
    let title = title.trim();
    if !title.is_empty() {
        return title.to_string();
    }
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .filter(|h| !h.is_empty())
        .unwrap_or_default()
}

fn add_source(sources: &mut Vec<WebSearchSource>, url: &str, title: &str, index: Option<u32>) {
    let url = url.trim();
    if url.is_empty() || !(url.starts_with("http://") || url.starts_with("https://")) {
        if !url.is_empty() {
            warn!(
                "web_search DeepSeek source ignored non-http url={}",
                truncate_for_log(url, 120)
            );
        }
        return;
    }
    let key = normalize_url_key(url);
    if let Some(existing) = sources
        .iter_mut()
        .find(|s| normalize_url_key(&s.url) == key)
    {
        let incoming = title.trim();
        let host = reqwest::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string));
        let host_only = existing.title.trim().is_empty()
            || host.is_some_and(|host| host.eq_ignore_ascii_case(existing.title.trim()));
        if !incoming.is_empty() && host_only {
            existing.title = incoming.to_string();
        }
        return;
    }
    let index = index.filter(|n| *n > 0).unwrap_or_else(|| {
        sources
            .iter()
            .map(|s| s.index)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    });
    sources.push(WebSearchSource {
        index,
        title: title_or_host(title, url),
        url: url.to_string(),
        site_name: None,
    });
}

fn push_source_value(sources: &mut Vec<WebSearchSource>, src: &Value) {
    let url = src
        .get("url")
        .and_then(|v| v.as_str())
        .or_else(|| src.get("source_website_url").and_then(|v| v.as_str()))
        .unwrap_or("");
    let title = src
        .get("title")
        .and_then(|v| v.as_str())
        .or_else(|| src.get("name").and_then(|v| v.as_str()))
        .unwrap_or("");
    let index = src.get("index").and_then(|v| v.as_u64()).map(|n| n as u32);
    add_source(sources, url, title, index);
}

fn collect_annotations(sources: &mut Vec<WebSearchSource>, part: &Value) {
    let Some(items) = part.get("annotations").and_then(|a| a.as_array()) else {
        return;
    };
    for ann in items {
        let ty = ann.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if ty != "url_citation" {
            if ann.get("url").is_some() {
                warn!("web_search DeepSeek skipped annotation type={ty}");
            }
            continue;
        }
        push_source_value(sources, ann);
    }
}

fn collect_item_sources(sources: &mut Vec<WebSearchSource>, item: &Value) {
    let ty = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
    if ty == "web_search_call" {
        for key in ["sources", "results"] {
            if let Some(items) = item.get(key).and_then(|s| s.as_array()) {
                for src in items {
                    push_source_value(sources, src);
                }
            }
        }
        if let Some(action) = item.get("action") {
            if let Some(items) = action.get("sources").and_then(|s| s.as_array()) {
                for src in items {
                    push_source_value(sources, src);
                }
            }
            if let Some(items) = action.get("results").and_then(|s| s.as_array()) {
                for src in items {
                    push_source_value(sources, src);
                }
            }
        }
    }
    if ty == "message" {
        match item.get("content") {
            Some(Value::Array(parts)) => {
                for part in parts {
                    collect_annotations(sources, part);
                }
            }
            Some(other) => collect_annotations(sources, other),
            None => {}
        }
    }
}

fn output_items(body: &Value) -> Option<&Vec<Value>> {
    body.get("output").and_then(|o| o.as_array())
}

fn message_text(body: &Value) -> String {
    let mut text = String::new();
    if let Some(items) = output_items(body) {
        for item in items {
            if item.get("type").and_then(|t| t.as_str()) != Some("message") {
                continue;
            }
            match item.get("content") {
                Some(Value::String(s)) => text.push_str(s),
                Some(Value::Array(parts)) => {
                    for part in parts {
                        let ty = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        if ty == "output_text" || ty.is_empty() {
                            if let Some(s) = part.get("text").and_then(|t| t.as_str()) {
                                text.push_str(s);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    if text.is_empty() {
        if let Some(s) = body.get("output_text").and_then(|v| v.as_str()) {
            text.push_str(s);
        }
    }
    text
}

fn read_usage(body: &Value) -> (WebSearchUsage, Option<u32>) {
    let Some(usage) = body.get("usage") else {
        return (WebSearchUsage::default(), None);
    };
    let input_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let output_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let mut total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    if total_tokens == 0 && (input_tokens > 0 || output_tokens > 0) {
        total_tokens = input_tokens.saturating_add(output_tokens);
        info!("web_search DeepSeek usage total_tokens missing; summed input and output");
    }
    let search_count = usage
        .pointer("/x_tools/web_search/count")
        .and_then(|c| c.as_u64())
        .map(|c| c as u32);
    (
        WebSearchUsage {
            input_tokens,
            output_tokens,
            total_tokens,
        },
        search_count,
    )
}

fn failure_message(body: &Value) -> Option<String> {
    let status = body.get("status").and_then(|s| s.as_str()).unwrap_or("");
    let err = body.get("error").filter(|e| !e.is_null());
    if status != "failed" && err.is_none() {
        return None;
    }
    let detail = err
        .and_then(|e| {
            e.get("message")
                .and_then(|m| m.as_str())
                .map(str::to_string)
                .or_else(|| e.as_str().map(str::to_string))
        })
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "DeepSeek web search failed".to_string());
    let code = err
        .and_then(|e| e.get("code"))
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty());
    Some(match code {
        Some(code) => format!("{code}: {detail}"),
        None => detail,
    })
}

fn note_call(acc: &mut DeepSeekAccum, item_id: Option<&str>) {
    match item_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => {
            acc.call_ids.insert(id.to_string());
        }
        None => {
            acc.anonymous_calls = acc.anonymous_calls.saturating_add(1);
        }
    }
}

fn merge_response(acc: &mut DeepSeekAccum, body: &Value, replace_text: bool) -> Result<bool> {
    if let Some(msg) = failure_message(body) {
        return Err(anyhow!("DeepSeek web search failed: {msg}"));
    }
    let status = body.get("status").and_then(|s| s.as_str()).unwrap_or("");
    if status == "incomplete" {
        warn!(
            "web_search DeepSeek response incomplete id={}",
            body.get("id").and_then(|v| v.as_str()).unwrap_or("-")
        );
    }
    let text = message_text(body);
    if replace_text && !text.is_empty() && !acc.saw_text {
        acc.answer = text;
        acc.saw_text = true;
    }
    let mut sources = Vec::new();
    if let Some(items) = output_items(body) {
        for item in items {
            if item.get("type").and_then(|t| t.as_str()) == Some("web_search_call") {
                note_call(acc, item.get("id").and_then(|v| v.as_str()));
            }
            collect_item_sources(&mut sources, item);
        }
    }
    let sources_changed = !sources.is_empty() && sources != acc.sources;
    if !sources.is_empty() {
        acc.sources = sources;
    }
    let (usage, search_count) = read_usage(body);
    if usage.total_tokens > 0 || usage.input_tokens > 0 || usage.output_tokens > 0 {
        acc.usage = usage;
    }
    if search_count.is_some() {
        acc.usage_search_count = search_count;
    }
    if let Some(id) = body
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        acc.request_id = Some(id.to_string());
    }
    Ok(sources_changed)
}

fn emit_sources(ui: Option<&WebSearchStreamUi>, acc: &DeepSeekAccum) {
    let Some(ui) = ui else {
        return;
    };
    if acc.sources.is_empty() {
        return;
    }
    let count = acc
        .usage_search_count
        .unwrap_or(if acc.sources.is_empty() { 0 } else { 1 });
    ui.emit_sources_ready(&acc.sources, count.max(1));
}

fn apply_event(
    acc: &mut DeepSeekAccum,
    event_name: &str,
    data: &Value,
    ui: Option<&WebSearchStreamUi>,
) -> Result<()> {
    let ty = data
        .get("type")
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(event_name);

    if ty == "response.failed" || event_name == "response.failed" {
        let body = data.get("response").unwrap_or(data);
        if let Some(msg) = failure_message(body) {
            return Err(anyhow!("DeepSeek web search failed: {msg}"));
        }
        return Err(anyhow!("DeepSeek web search failed"));
    }

    if ty == "response.output_text.delta" {
        let delta = data.get("delta").and_then(|d| d.as_str()).unwrap_or("");
        if !delta.is_empty() {
            acc.answer.push_str(delta);
            acc.saw_text = true;
            if let Some(ui) = ui {
                ui.emit_output_delta(delta);
            }
        } else {
            warn!("web_search DeepSeek output_text.delta had no delta text");
        }
        return Ok(());
    }

    if ty == "response.output_text.done" {
        let text = data.get("text").and_then(|t| t.as_str()).unwrap_or("");
        if !acc.saw_text && !text.is_empty() {
            acc.answer = text.to_string();
            acc.saw_text = true;
            if let Some(ui) = ui {
                ui.emit_output_delta(text);
            }
        }
        return Ok(());
    }

    if ty == "response.web_search_call.in_progress"
        || ty == "response.web_search_call.searching"
        || ty == "response.web_search_call.completed"
    {
        info!("web_search DeepSeek call event={ty}");
        if ty == "response.web_search_call.completed" {
            if let Some(item_id) = data
                .get("item_id")
                .and_then(|v| v.as_str())
                .or_else(|| data.get("id").and_then(|v| v.as_str()))
            {
                note_call(acc, Some(item_id));
            }
        }
        if let Some(item) = data.get("item") {
            let mut sources = acc.sources.clone();
            collect_item_sources(&mut sources, item);
            if sources != acc.sources {
                acc.sources = sources;
                emit_sources(ui, acc);
            }
        }
        return Ok(());
    }

    if ty == "response.output_item.done" || ty == "response.output_item.added" {
        let Some(item) = data.get("item") else {
            warn!("web_search DeepSeek {ty} missing item");
            return Ok(());
        };
        if item.get("type").and_then(|t| t.as_str()) == Some("web_search_call") {
            if ty.ends_with("done") {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    note_call(acc, Some(id));
                }
            }
            let mut sources = acc.sources.clone();
            collect_item_sources(&mut sources, item);
            if sources != acc.sources {
                acc.sources = sources;
                emit_sources(ui, acc);
            }
        }
        return Ok(());
    }

    if ty == "response.completed" || ty == "response.incomplete" || data.get("output").is_some() {
        let body = data.get("response").unwrap_or(data);
        let had_text = acc.saw_text;
        let changed = merge_response(acc, body, true)?;
        if changed {
            emit_sources(ui, acc);
        }
        if !had_text && !acc.answer.is_empty() {
            if let Some(ui) = ui {
                ui.emit_output_delta(&acc.answer);
            }
        }
        return Ok(());
    }

    if ty.starts_with("response.reasoning")
        || ty.starts_with("response.content_part")
        || ty == "response.created"
        || ty == "response.in_progress"
        || ty.starts_with("response.function_call")
    {
        return Ok(());
    }
    if !ty.is_empty() {
        warn!("web_search DeepSeek unhandled event={ty}");
    }
    Ok(())
}

fn apply_sse_block(
    acc: &mut DeepSeekAccum,
    block: &str,
    ui: Option<&WebSearchStreamUi>,
) -> Result<()> {
    let mut event_name = String::new();
    let mut data = String::new();
    for line in block.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("event:") {
            event_name = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.trim());
        }
    }
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }
    let parsed: Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(e) => {
            warn!("web_search DeepSeek SSE JSON parse failed: {e}");
            return Ok(());
        }
    };
    apply_event(acc, &event_name, &parsed, ui)
}

fn parse_deepseek_responses_sse(
    body: &str,
    ui: Option<&WebSearchStreamUi>,
) -> Result<DeepSeekAccum> {
    let mut acc = DeepSeekAccum::default();
    let mut block = String::new();
    for line in body.split_inclusive('\n') {
        if line.trim().is_empty() {
            if !block.trim().is_empty() {
                apply_sse_block(&mut acc, &block, ui)?;
                block.clear();
            }
            continue;
        }
        block.push_str(line);
    }
    if !block.trim().is_empty() {
        apply_sse_block(&mut acc, &block, ui)?;
    }
    Ok(acc)
}

pub fn parse_deepseek_response(
    query: &str,
    model: &str,
    search_strategy: &str,
    body: &Value,
    ui: Option<&WebSearchStreamUi>,
) -> Result<WebSearchResult> {
    if body.get("output").is_none()
        && body.get("status").is_none()
        && body.get("output_text").is_none()
        && body.get("error").is_none()
    {
        return Err(anyhow!(
            "DeepSeek web search returned an unrecognized response"
        ));
    }
    let mut acc = DeepSeekAccum::default();
    let changed = merge_response(&mut acc, body, true)?;
    if !acc.saw_text {
        let text = message_text(body);
        if !text.is_empty() {
            acc.answer = text;
            acc.saw_text = true;
        }
    }
    if acc.answer.is_empty() && acc.sources.is_empty() {
        warn!(
            "web_search DeepSeek response had no answer and no sources id={}",
            acc.request_id.as_deref().unwrap_or("-")
        );
    }
    if changed || !acc.sources.is_empty() {
        emit_sources(ui, &acc);
    }
    if !acc.answer.is_empty() {
        if let Some(ui) = ui {
            ui.emit_output_delta(&acc.answer);
        }
    }
    Ok(acc.into_result(query, model, search_strategy))
}

fn provider_has_key(provider: &ProviderConfig) -> bool {
    !provider.api_key.trim().is_empty()
}

fn find_provider<'a>(settings: &'a ModelSettings, id: &str) -> Option<&'a ProviderConfig> {
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    settings.providers.iter().find(|p| p.id == id)
}

fn active_provider(settings: &ModelSettings) -> Option<&ProviderConfig> {
    find_provider(settings, &settings.active_provider_id).or_else(|| settings.providers.first())
}

fn deepseek_with_key<'a>(
    settings: &'a ModelSettings,
    preferred_id: Option<&str>,
) -> Option<&'a ProviderConfig> {
    if let Some(id) = preferred_id {
        if let Some(p) = find_provider(settings, id) {
            if provider_is_deepseek_search(p) && provider_has_key(p) {
                return Some(p);
            }
        }
    }
    if let Some(p) = active_provider(settings) {
        if provider_is_deepseek_search(p) && provider_has_key(p) {
            return Some(p);
        }
    }
    settings
        .providers
        .iter()
        .find(|p| provider_is_deepseek_search(p) && provider_has_key(p))
}

fn dashscope_with_key(settings: &ModelSettings) -> bool {
    settings
        .providers
        .iter()
        .any(|p| provider_uses_dashscope_compatible_api(p) && provider_has_key(p))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SearchChoice {
    DeepSeek { provider_id: String, model: String },
    DashScope { model: String },
}

fn choice_from_provider_and_model(
    settings: &ModelSettings,
    provider_id: &str,
    model: &str,
) -> SearchChoice {
    let model_l = model.trim().to_ascii_lowercase();
    if let Some(p) = find_provider(settings, provider_id) {
        if provider_is_deepseek_search(p) || model_l.starts_with("deepseek") {
            return SearchChoice::DeepSeek {
                provider_id: p.id.clone(),
                model: model.trim().to_string(),
            };
        }
        if provider_uses_dashscope_compatible_api(p) {
            return SearchChoice::DashScope {
                model: model.trim().to_string(),
            };
        }
    }
    if provider_id.eq_ignore_ascii_case("deepseek") || model_l.starts_with("deepseek") {
        return SearchChoice::DeepSeek {
            provider_id: provider_id.trim().to_string(),
            model: model.trim().to_string(),
        };
    }
    SearchChoice::DashScope {
        model: model.trim().to_string(),
    }
}

fn explicit_search_choice(
    settings: &ModelSettings,
    env_model: Option<&str>,
) -> Option<SearchChoice> {
    if let Some(model) = env_model.map(str::trim).filter(|s| !s.is_empty()) {
        info!("web_search explicit model from POINTER_WEB_SEARCH_MODEL={model}");
        return Some(choice_from_provider_and_model(settings, "", model));
    }
    let mode = crate::mode_llm::agent_performance_mode(settings, WEB_SEARCH_SCENE_ID);
    if let Some(cfg) = settings
        .agent_mode_llm
        .get(WEB_SEARCH_SCENE_ID)
        .and_then(|inner| inner.get(mode))
    {
        let model = cfg.model.trim();
        if !model.is_empty() {
            return Some(choice_from_provider_and_model(
                settings,
                &cfg.provider_id,
                model,
            ));
        }
    }
    let legacy = settings.web_search_model.trim();
    if !legacy.is_empty() {
        return Some(choice_from_provider_and_model(settings, "", legacy));
    }
    None
}

fn route_deepseek(
    settings: &ModelSettings,
    provider_id: &str,
    model: &str,
) -> Result<WebSearchRoute> {
    let provider = deepseek_with_key(settings, Some(provider_id).filter(|s| !s.is_empty()));
    let Some(provider) = provider else {
        if dashscope_with_key(settings) {
            warn!(
                "web_search DeepSeek API key missing for model={}; falling back to DashScope",
                if model.is_empty() {
                    DEEPSEEK_WEB_SEARCH_MODEL
                } else {
                    model
                }
            );
            return Ok(WebSearchRoute::DashScope);
        }
        return Err(anyhow!(
            "DeepSeek API key is missing. Configure the DeepSeek provider API key in settings."
        ));
    };
    Ok(WebSearchRoute::DeepSeek {
        provider_id: provider.id.clone(),
        requested_model: model.to_string(),
    })
}

pub(crate) fn resolve_web_search_route_with_env(
    settings: &ModelSettings,
    env_model: Option<&str>,
) -> Result<WebSearchRoute> {
    if let Some(choice) = explicit_search_choice(settings, env_model) {
        return match choice {
            SearchChoice::DeepSeek { provider_id, model } => {
                info!(
                    "web_search backend=deepseek reason=explicit provider={} model={}",
                    provider_id, model
                );
                route_deepseek(settings, &provider_id, &model)
            }
            SearchChoice::DashScope { model } => {
                if dashscope_with_key(settings) {
                    info!("web_search backend=dashscope reason=explicit model={model}");
                    return Ok(WebSearchRoute::DashScope);
                }
                if deepseek_with_key(settings, None).is_some() {
                    warn!(
                        "web_search DashScope API key missing for model={model}; falling back to DeepSeek"
                    );
                    return route_deepseek(settings, "", "");
                }
                info!("web_search backend=dashscope reason=explicit model={model} key=missing");
                Ok(WebSearchRoute::DashScope)
            }
        };
    }

    if let Some(active) = active_provider(settings) {
        if provider_is_deepseek_search(active) {
            if provider_has_key(active) {
                let model = settings.model.trim().to_string();
                info!(
                    "web_search backend=deepseek reason=active_provider provider={} model={model}",
                    active.id
                );
                return Ok(WebSearchRoute::DeepSeek {
                    provider_id: active.id.clone(),
                    requested_model: model,
                });
            }
            if dashscope_with_key(settings) {
                warn!(
                    "web_search active DeepSeek provider {} has no API key; falling back to DashScope",
                    active.id
                );
                return Ok(WebSearchRoute::DashScope);
            }
            return Err(anyhow!(
                "DeepSeek API key is missing. Configure the DeepSeek provider API key in settings."
            ));
        }
    }

    if dashscope_with_key(settings) {
        info!("web_search backend=dashscope reason=provider_key");
        return Ok(WebSearchRoute::DashScope);
    }
    if deepseek_with_key(settings, None).is_some() {
        info!("web_search backend=deepseek reason=only_configured_key");
        return route_deepseek(settings, "", "");
    }
    info!("web_search backend=dashscope reason=no_search_key");
    Ok(WebSearchRoute::DashScope)
}

pub(crate) async fn execute_routed_web_search(
    settings: &ModelSettings,
    agent_id: Option<&str>,
    req: WebSearchRequest,
    cancel: CancellationToken,
    ui: Option<WebSearchStreamUi>,
    env_model: Option<&str>,
) -> Result<WebSearchResult> {
    match resolve_web_search_route_with_env(settings, env_model)? {
        WebSearchRoute::DashScope => {
            super::generation::execute_generation_web_search(settings, agent_id, req, cancel, ui)
                .await
        }
        WebSearchRoute::DeepSeek {
            provider_id,
            requested_model,
        } => {
            execute_deepseek_web_search(settings, &provider_id, &requested_model, req, cancel, ui)
                .await
        }
    }
}

pub(crate) async fn execute_deepseek_web_search(
    settings: &ModelSettings,
    provider_id: &str,
    requested_model: &str,
    req: WebSearchRequest,
    cancel: CancellationToken,
    ui: Option<WebSearchStreamUi>,
) -> Result<WebSearchResult> {
    let provider = find_provider(settings, provider_id)
        .filter(|p| provider_is_deepseek_search(p))
        .ok_or_else(|| {
            anyhow!("DeepSeek provider {provider_id} is not configured for web search")
        })?;
    let api_key = provider.api_key.trim();
    if api_key.is_empty() {
        return Err(anyhow!(
            "DeepSeek API key is missing. Configure the DeepSeek provider API key in settings."
        ));
    }
    let (model, substituted) = resolve_deepseek_web_search_model(requested_model);
    if substituted {
        warn!(
            "web_search DeepSeek model {} ignores web_search; calling {}",
            requested_model.trim(),
            model
        );
    }
    let url = deepseek_responses_url(&provider.base_url);
    let body = build_deepseek_web_search_body(&model, &req);
    info!(
        "web_search DeepSeek start model={model} url={url} query={} thinking={} strategy_ignored={}",
        truncate_for_log(&req.query, 80),
        req.enable_thinking,
        req.search_strategy
    );
    log_web_search_http_request(&url, &body, true);
    let started = Instant::now();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(DEFAULT_WEB_SEARCH_TIMEOUT_SECS))
        .build()
        .context("failed to build HTTP client for DeepSeek web search")?;

    let send = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .json(&body);

    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(anyhow!(crate::i18n::generation_stopped_msg())),
        r = send.send() => r.context("DeepSeek web search HTTP request failed")?,
    };

    let status = resp.status();
    if !status.is_success() {
        let text = resp
            .text()
            .await
            .context("failed to read DeepSeek web search error body")?;
        warn!(
            "web_search DeepSeek error status={} url={url} query={} response={}",
            status,
            truncate_for_log(&req.query, 80),
            truncate_for_log(&text, 400)
        );
        return Err(anyhow!(
            "DeepSeek web search failed (HTTP {}): {}",
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
        let acc = read_deepseek_sse(resp.bytes_stream(), &cancel, ui.as_ref()).await?;
        if acc.answer.is_empty() && acc.sources.is_empty() {
            warn!("web_search DeepSeek SSE ended with no answer and no sources");
        }
        acc.into_result(&req.query, &model, &req.search_strategy)
    } else {
        let text = resp
            .text()
            .await
            .context("failed to read DeepSeek web search response body")?;
        let trimmed = text.trim_start();
        if trimmed.starts_with("event:") || trimmed.starts_with("data:") {
            let acc = parse_deepseek_responses_sse(&text, ui.as_ref())?;
            acc.into_result(&req.query, &model, &req.search_strategy)
        } else {
            let parsed: Value =
                serde_json::from_str(&text).context("DeepSeek web search returned invalid JSON")?;
            parse_deepseek_response(
                &req.query,
                &model,
                &req.search_strategy,
                &parsed,
                ui.as_ref(),
            )?
        }
    };

    info!(
        "web_search DeepSeek ok model={model} url={url} query={} sources={} search_count={} latency_ms={} request_id={:?}",
        truncate_for_log(&req.query, 80),
        result.sources.len(),
        result.search_count,
        started.elapsed().as_millis(),
        result.request_id
    );
    Ok(result)
}

async fn read_deepseek_sse<S, E>(
    mut byte_stream: S,
    cancel: &CancellationToken,
    ui: Option<&WebSearchStreamUi>,
) -> Result<DeepSeekAccum>
where
    S: futures_util::Stream<Item = Result<bytes::Bytes, E>> + Unpin,
    E: std::error::Error + Send + Sync + 'static,
{
    use futures_util::StreamExt;

    let mut acc = DeepSeekAccum::default();
    let mut buf = String::new();
    loop {
        let item = tokio::select! {
            _ = cancel.cancelled() => return Err(anyhow!(crate::i18n::generation_stopped_msg())),
            v = byte_stream.next() => v,
        };
        let chunk = match item {
            Some(Ok(c)) => c,
            Some(Err(e)) => return Err(anyhow!("DeepSeek web search SSE read failed: {e}")),
            None => break,
        };
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find("\n\n") {
            let block = buf[..pos].to_string();
            buf.drain(..pos + 2);
            apply_sse_block(&mut acc, &block, ui)?;
        }
    }
    if !buf.trim().is_empty() {
        apply_sse_block(&mut acc, &buf, ui)?;
    }
    Ok(acc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ComputerTierLlmConfig;
    use serde_json::json;
    use std::collections::HashMap;

    fn provider(id: &str, base: &str, key: &str, models: &[&str]) -> ProviderConfig {
        serde_json::from_value(json!({
            "id": id,
            "name": id,
            "baseUrl": base,
            "apiKey": key,
            "models": models,
        }))
        .unwrap()
    }

    fn settings(providers: Vec<ProviderConfig>, active: &str, model: &str) -> ModelSettings {
        ModelSettings {
            providers,
            active_provider_id: active.into(),
            model: model.into(),
            ..ModelSettings::default()
        }
    }

    fn sample_req() -> WebSearchRequest {
        WebSearchRequest {
            query: "latest stable Rust edition".into(),
            search_strategy: "pro_max".into(),
            forced_search: true,
            enable_vertical_search: true,
            enable_thinking: false,
            messages: vec![],
        }
    }

    #[test]
    fn responses_url_strips_v1() {
        assert_eq!(
            deepseek_responses_url("https://api.deepseek.com/v1"),
            "https://api.deepseek.com/responses"
        );
        assert_eq!(
            deepseek_responses_url("https://api.deepseek.com"),
            "https://api.deepseek.com/responses"
        );
        assert_eq!(
            deepseek_responses_url("http://127.0.0.1:9/v1/"),
            "http://127.0.0.1:9/responses"
        );
    }

    #[test]
    fn flash_is_rewritten_to_v4_pro() {
        let (model, substituted) = resolve_deepseek_web_search_model("deepseek-flash");
        assert_eq!(model, "deepseek-v4-pro");
        assert!(substituted);
        let (model, substituted) = resolve_deepseek_web_search_model("deepseek-v4-pro");
        assert_eq!(model, "deepseek-v4-pro");
        assert!(!substituted);
        let (model, substituted) = resolve_deepseek_web_search_model("deepseek-v4-pro-2026-08-01");
        assert_eq!(model, "deepseek-v4-pro-2026-08-01");
        assert!(!substituted);
        let (model, substituted) = resolve_deepseek_web_search_model("");
        assert_eq!(model, "deepseek-v4-pro");
        assert!(!substituted);
    }

    #[test]
    fn request_body_uses_web_search_tool_and_omits_dashscope_fields() {
        let body = build_deepseek_web_search_body("deepseek-v4-pro", &sample_req());
        assert_eq!(body["model"], "deepseek-v4-pro");
        assert_eq!(body["input"], "latest stable Rust edition");
        assert_eq!(body["tools"][0]["type"], "web_search");
        assert_eq!(body["stream"], true);
        assert!(body.get("search_strategy").is_none());
        assert!(body.get("reasoning").is_none());
        assert!(body.get("tool_choice").is_none());

        let mut thinking = sample_req();
        thinking.enable_thinking = true;
        let body = build_deepseek_web_search_body("deepseek-v4-pro", &thinking);
        assert_eq!(body["reasoning"]["effort"], "high");
    }

    #[test]
    fn parses_citations_and_search_call_sources() {
        let body = json!({
            "id": "resp_test",
            "status": "completed",
            "output": [
                {
                    "type": "web_search_call",
                    "id": "ws_1",
                    "status": "completed",
                    "action": {
                        "type": "search",
                        "sources": [
                            { "type": "url", "url": "https://doc.rust-lang.org/edition-guide/" }
                        ]
                    }
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": "The 2024 edition is stable.",
                        "annotations": [{
                            "type": "url_citation",
                            "url": "https://doc.rust-lang.org/edition-guide/",
                            "title": "Rust Edition Guide"
                        }, {
                            "type": "url_citation",
                            "url": "https://blog.rust-lang.org/2024/02/08/Rust-1.76.0.html",
                            "title": "Announcing Rust 1.76.0"
                        }]
                    }]
                }
            ],
            "usage": {
                "input_tokens": 20,
                "output_tokens": 8,
                "total_tokens": 28,
                "x_tools": { "web_search": { "count": 1 } }
            }
        });
        let result =
            parse_deepseek_response("rust", "deepseek-v4-pro", "pro_max", &body, None).unwrap();
        assert!(result.ok);
        assert_eq!(result.answer, "The 2024 edition is stable.");
        assert_eq!(result.sources.len(), 2);
        assert_eq!(result.sources[0].title, "Rust Edition Guide");
        assert_eq!(
            result.sources[0].url,
            "https://doc.rust-lang.org/edition-guide/"
        );
        assert_eq!(result.search_count, 1);
        assert_eq!(result.usage.total_tokens, 28);
        assert_eq!(result.request_id.as_deref(), Some("resp_test"));
    }

    #[test]
    fn failed_response_is_an_error() {
        let body = json!({
            "id": "resp_bad",
            "status": "failed",
            "error": { "code": "server_error", "message": "search backend down" }
        });
        let err =
            parse_deepseek_response("q", "deepseek-v4-pro", "pro_max", &body, None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("search backend down"));
        assert!(msg.contains("server_error"));
    }

    #[test]
    fn sse_deltas_then_completed_sources() {
        let completed = json!({
            "type": "response.completed",
            "response": {
                "id": "resp_sse",
                "status": "completed",
                "output": [
                    {
                        "type": "web_search_call",
                        "id": "ws_9",
                        "action": { "sources": [{ "url": "https://example.com/a", "title": "A" }] }
                    },
                    {
                        "type": "message",
                        "content": [{ "type": "output_text", "text": "Hello world" }]
                    }
                ],
                "usage": { "input_tokens": 3, "output_tokens": 2, "total_tokens": 5 }
            }
        });
        let sse = format!(
            "event: response.output_text.delta\ndata: {{\"type\":\"response.output_text.delta\",\"delta\":\"Hello \"}}\n\n\
             event: response.output_text.delta\ndata: {{\"type\":\"response.output_text.delta\",\"delta\":\"world\"}}\n\n\
             event: response.completed\ndata: {completed}\n\n"
        );
        let acc = parse_deepseek_responses_sse(&sse, None).unwrap();
        let result = acc.into_result("q", "deepseek-v4-pro", "pro_max");
        assert_eq!(result.answer, "Hello world");
        assert_eq!(result.sources.len(), 1);
        assert_eq!(result.sources[0].title, "A");
        assert_eq!(result.request_id.as_deref(), Some("resp_sse"));
        assert_eq!(result.usage.total_tokens, 5);
    }

    #[test]
    fn active_deepseek_routes_to_deepseek() {
        let s = settings(
            vec![provider(
                "deepseek",
                "https://api.deepseek.com/v1",
                "sk",
                &["deepseek-flash", "deepseek-v4-pro"],
            )],
            "deepseek",
            "deepseek-flash",
        );
        let route = resolve_web_search_route_with_env(&s, None).unwrap();
        assert_eq!(
            route,
            WebSearchRoute::DeepSeek {
                provider_id: "deepseek".into(),
                requested_model: "deepseek-flash".into(),
            }
        );
    }

    #[test]
    fn active_qwen_keeps_dashscope() {
        let s = settings(
            vec![
                provider(
                    "qwen",
                    "https://dashscope.aliyuncs.com/compatible-mode/v1",
                    "sk-qwen",
                    &["qwen3.8-max"],
                ),
                provider(
                    "deepseek",
                    "https://api.deepseek.com/v1",
                    "sk-ds",
                    &["deepseek-v4-pro"],
                ),
            ],
            "qwen",
            "qwen3.8-max",
        );
        let route = resolve_web_search_route_with_env(&s, None).unwrap();
        assert_eq!(route, WebSearchRoute::DashScope);
    }

    #[test]
    fn explicit_tier_overrides_active_provider() {
        let mut s = settings(
            vec![
                provider(
                    "deepseek",
                    "https://api.deepseek.com/v1",
                    "sk-ds",
                    &["deepseek-v4-pro"],
                ),
                provider(
                    "qwen",
                    "https://dashscope.aliyuncs.com/compatible-mode/v1",
                    "sk-qwen",
                    &["qwen3.8-max"],
                ),
            ],
            "deepseek",
            "deepseek-v4-pro",
        );
        let mut fast = HashMap::new();
        fast.insert(
            "fast".into(),
            ComputerTierLlmConfig {
                provider_id: "qwen".into(),
                model: "qwen3.8-max".into(),
                enable_thinking: false,
                thinking_budget: None,
                reasoning_effort: None,
                thinking_intensity: None,
            },
        );
        s.agent_mode_llm.insert(WEB_SEARCH_SCENE_ID.into(), fast);
        let route = resolve_web_search_route_with_env(&s, None).unwrap();
        assert_eq!(route, WebSearchRoute::DashScope);

        let route = resolve_web_search_route_with_env(&s, Some("deepseek-v4-pro")).unwrap();
        assert_eq!(
            route,
            WebSearchRoute::DeepSeek {
                provider_id: "deepseek".into(),
                requested_model: "deepseek-v4-pro".into(),
            }
        );
    }

    #[test]
    fn deepseek_only_key_is_used_when_dashscope_tier_has_no_key() {
        let mut s = settings(
            vec![provider(
                "deepseek",
                "https://api.deepseek.com/v1",
                "sk-ds",
                &["deepseek-v4-pro"],
            )],
            "kimi",
            "kimi-k3",
        );
        s.web_search_model = "qwen3.8-max".into();
        let route = resolve_web_search_route_with_env(&s, None).unwrap();
        assert_eq!(
            route,
            WebSearchRoute::DeepSeek {
                provider_id: "deepseek".into(),
                requested_model: String::new(),
            }
        );
    }

    /// A DeepSeek key alone, plus the web-search fast tier, must not fall
    /// through to the DashScope "provider not configured" error.
    #[test]
    fn deepseek_key_and_tier_never_routes_to_dashscope() {
        let mut s = settings(
            vec![provider(
                "deepseek",
                "https://api.deepseek.com/v1",
                "sk-ds",
                &["deepseek-v4-pro", "deepseek-flash"],
            )],
            "deepseek",
            "deepseek-v4-pro",
        );
        s.web_search_model.clear();
        let mut fast = HashMap::new();
        fast.insert(
            "fast".into(),
            ComputerTierLlmConfig {
                provider_id: "deepseek".into(),
                model: "deepseek-v4-pro".into(),
                enable_thinking: true,
                thinking_budget: None,
                reasoning_effort: None,
                thinking_intensity: None,
            },
        );
        s.agent_mode_llm.insert(WEB_SEARCH_SCENE_ID.into(), fast);
        let route = resolve_web_search_route_with_env(&s, None).unwrap();
        assert_eq!(
            route,
            WebSearchRoute::DeepSeek {
                provider_id: "deepseek".into(),
                requested_model: "deepseek-v4-pro".into(),
            }
        );
    }

    #[tokio::test]
    async fn http_json_response_parses_sources() {
        use wiremock::matchers::{body_partial_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .and(body_partial_json(json!({
                "model": "deepseek-v4-pro",
                "tools": [{ "type": "web_search" }],
                "stream": true
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "resp_http",
                "status": "completed",
                "output": [
                    {
                        "type": "message",
                        "content": [{
                            "type": "output_text",
                            "text": "Rust 2024 is stable.",
                            "annotations": [{
                                "type": "url_citation",
                                "url": "https://doc.rust-lang.org/edition-guide/",
                                "title": "Edition Guide"
                            }]
                        }]
                    }
                ],
                "usage": { "input_tokens": 4, "output_tokens": 4, "total_tokens": 8 }
            })))
            .mount(&server)
            .await;

        let s = settings(
            vec![provider(
                "deepseek",
                &format!("{}/v1", server.uri()),
                "sk-test",
                &["deepseek-flash"],
            )],
            "deepseek",
            "deepseek-flash",
        );
        let result =
            execute_routed_web_search(&s, None, sample_req(), CancellationToken::new(), None, None)
                .await
                .unwrap();
        assert_eq!(result.model, "deepseek-v4-pro");
        assert_eq!(result.answer, "Rust 2024 is stable.");
        assert_eq!(result.sources.len(), 1);
        assert_eq!(result.sources[0].title, "Edition Guide");
        assert_eq!(result.request_id.as_deref(), Some("resp_http"));
    }

    #[tokio::test]
    async fn http_error_includes_status() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(401).set_body_string("bad key"))
            .mount(&server)
            .await;

        let s = settings(
            vec![provider(
                "deepseek",
                &format!("{}/v1", server.uri()),
                "sk-test",
                &["deepseek-v4-pro"],
            )],
            "deepseek",
            "deepseek-v4-pro",
        );
        let err =
            execute_routed_web_search(&s, None, sample_req(), CancellationToken::new(), None, None)
                .await
                .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("401"), "{msg}");
    }
}
