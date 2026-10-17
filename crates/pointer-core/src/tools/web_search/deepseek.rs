//! DeepSeek Search API (`POST /v1/tools/search`).
//!
//! Official docs: https://api-docs.deepseek.com/zh-cn/guides/search
//! The call returns structured pages. It does not run a chat model.

use crate::models::{
    provider_uses_dashscope_compatible_api, provider_uses_deepseek_api, ModelSettings,
    ProviderConfig, WEB_SEARCH_SCENE_ID,
};
use anyhow::{anyhow, Context, Result};
use log::{info, warn};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use super::client::{
    log_web_search_http_request, truncate_for_log, WebSearchRequest, WebSearchResult,
    WebSearchSource, WebSearchUsage,
};
use super::stream_ui::WebSearchStreamUi;

/// Label stored on the tool result. The Search API has no model id.
pub const DEEPSEEK_SEARCH_MODEL: &str = "deepseek-search";

const DEEPSEEK_SEARCH_COUNT: u64 = 10;
const DEEPSEEK_SEARCH_QUERY_MAX_CHARS: usize = 128;
const DEEPSEEK_SEARCH_TIMEOUT_SECS: u64 = 60;

/// `deepseek` preset id, or any provider whose base URL is the official host.
pub fn provider_is_deepseek_search(provider: &ProviderConfig) -> bool {
    provider.id.eq_ignore_ascii_case("deepseek") || provider_uses_deepseek_api(provider)
}

/// `https://api.deepseek.com/v1` and `https://api.deepseek.com` both become
/// `https://api.deepseek.com/v1/tools/search`.
pub fn deepseek_search_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return "https://api.deepseek.com/v1/tools/search".to_string();
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
        return "https://api.deepseek.com/v1/tools/search".to_string();
    }
    format!("{origin}/v1/tools/search")
}

fn strip_suffix_ci<'a>(original: &'a str, lower: &str, suffix: &str) -> Option<&'a str> {
    if lower.ends_with(suffix) {
        Some(&original[..original.len() - suffix.len()])
    } else {
        None
    }
}

fn prepare_search_query(query: &str) -> Result<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("DeepSeek search query is empty"));
    }
    let chars = trimmed.chars().count();
    if chars > DEEPSEEK_SEARCH_QUERY_MAX_CHARS {
        return Err(anyhow!(
            "DeepSeek search query is {chars} characters; the search API accepts at most {DEEPSEEK_SEARCH_QUERY_MAX_CHARS}. Shorten the query."
        ));
    }
    Ok(trimmed.to_string())
}

pub fn build_deepseek_search_body(query: &str) -> Value {
    json!({
        "query": query,
        "count": DEEPSEEK_SEARCH_COUNT,
        "need_content": false,
    })
}

pub fn parse_deepseek_search_response(
    query: &str,
    search_strategy: &str,
    body: &Value,
) -> Result<WebSearchResult> {
    if let Some(msg) = body
        .pointer("/error/message")
        .and_then(|m| m.as_str())
        .filter(|s| !s.is_empty())
    {
        return Err(anyhow!("DeepSeek search failed: {msg}"));
    }
    let Some(items) = body.get("results").and_then(|v| v.as_array()) else {
        return Err(anyhow!(
            "DeepSeek search response did not include a results array"
        ));
    };
    let mut sources = Vec::new();
    let mut answer = String::new();
    let mut seen_urls = std::collections::HashSet::new();
    for item in items {
        let url = item
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if url.is_empty() || !(url.starts_with("http://") || url.starts_with("https://")) {
            if !url.is_empty() {
                warn!(
                    "web_search DeepSeek source ignored non-http url={}",
                    truncate_for_log(url, 120)
                );
            }
            continue;
        }
        let url_key = url.trim_end_matches('/').to_ascii_lowercase();
        if !seen_urls.insert(url_key) {
            warn!(
                "web_search DeepSeek source ignored duplicate url={}",
                truncate_for_log(url, 120)
            );
            continue;
        }
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let summary = item
            .get("summary")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                item.get("snippet")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
            })
            .unwrap_or("")
            .trim();
        let site_name = item
            .get("site_name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let index = (sources.len() as u32).saturating_add(1);
        let title = if title.is_empty() {
            site_name.clone().unwrap_or_else(|| url.to_string())
        } else {
            title.to_string()
        };
        if !answer.is_empty() {
            answer.push_str("\n\n");
        }
        answer.push_str(&format!("{index}. {title}\n{url}"));
        if !summary.is_empty() {
            answer.push('\n');
            answer.push_str(summary);
        }
        sources.push(WebSearchSource {
            index,
            title,
            url: url.to_string(),
            site_name,
        });
    }
    let search_count = sources.len() as u32;
    if search_count == 0 {
        info!("web_search DeepSeek search returned no http results");
    }
    Ok(WebSearchResult {
        ok: true,
        query: query.to_string(),
        answer,
        sources,
        search_count,
        usage: WebSearchUsage::default(),
        model: DEEPSEEK_SEARCH_MODEL.to_string(),
        search_strategy: search_strategy.to_string(),
        request_id: None,
        citation_base_index: None,
    })
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
                if model.is_empty() { "(none)" } else { model }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WebSearchRoute {
    DashScope,
    DeepSeek {
        provider_id: String,
        requested_model: String,
    },
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
    let query = prepare_search_query(&req.query)?;
    let url = deepseek_search_url(&provider.base_url);
    let body = build_deepseek_search_body(&query);
    info!(
        "web_search DeepSeek search start url={url} query={} count={DEEPSEEK_SEARCH_COUNT} chat_model={}",
        truncate_for_log(&query, 80),
        if requested_model.trim().is_empty() {
            "-"
        } else {
            requested_model.trim()
        }
    );
    log_web_search_http_request(&url, &body, false);
    let started = Instant::now();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(DEEPSEEK_SEARCH_TIMEOUT_SECS))
        .build()
        .context("failed to build HTTP client for DeepSeek search")?;

    let send = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body);

    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(anyhow!(crate::i18n::generation_stopped_msg())),
        r = send.send() => r.context("DeepSeek search HTTP request failed")?,
    };

    let status = resp.status();
    let text = resp
        .text()
        .await
        .context("failed to read DeepSeek search response body")?;
    if !status.is_success() {
        let detail = search_error_detail(&text);
        warn!(
            "web_search DeepSeek search error status={} url={url} query={} response={}",
            status,
            truncate_for_log(&query, 80),
            truncate_for_log(&detail, 400)
        );
        return Err(anyhow!(
            "DeepSeek search failed (HTTP {}): {}",
            status.as_u16(),
            truncate_for_log(&detail, 400)
        ));
    }
    let parsed: Value =
        serde_json::from_str(&text).context("DeepSeek search returned invalid JSON")?;
    let result = parse_deepseek_search_response(&query, &req.search_strategy, &parsed)?;
    if let Some(ui) = ui.as_ref() {
        if !result.sources.is_empty() {
            ui.emit_sources_ready(&result.sources, result.search_count);
        }
        if !result.answer.is_empty() {
            ui.emit_output_delta(&result.answer);
        }
    }
    info!(
        "web_search DeepSeek search ok url={url} query={} sources={} latency_ms={}",
        truncate_for_log(&query, 80),
        result.sources.len(),
        started.elapsed().as_millis()
    );
    Ok(result)
}

fn search_error_detail(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .and_then(|m| m.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.trim().to_string())
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
    fn search_url_strips_v1() {
        assert_eq!(
            deepseek_search_url("https://api.deepseek.com/v1"),
            "https://api.deepseek.com/v1/tools/search"
        );
        assert_eq!(
            deepseek_search_url("https://api.deepseek.com"),
            "https://api.deepseek.com/v1/tools/search"
        );
        assert_eq!(
            deepseek_search_url("http://127.0.0.1:9/v1/"),
            "http://127.0.0.1:9/v1/tools/search"
        );
    }

    #[test]
    fn request_body_is_search_api() {
        let body = build_deepseek_search_body("DeepSeek site:github.com");
        assert_eq!(body["query"], "DeepSeek site:github.com");
        assert_eq!(body["count"], 10);
        assert_eq!(body["need_content"], false);
        assert!(body.get("model").is_none());
        assert!(body.get("tools").is_none());
        assert!(body.get("stream").is_none());
        assert!(body.get("search_strategy").is_none());
    }

    #[test]
    fn query_over_128_chars_is_rejected() {
        let err = prepare_search_query(&"a".repeat(129)).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("128"), "{msg}");
    }

    #[test]
    fn parses_results_into_sources_and_answer() {
        let body = json!({
            "results": [
                {
                    "rerank_score": 2.4,
                    "title": "DeepSeek V4",
                    "url": "https://example.com/deepseek-v4",
                    "snippet": "short",
                    "summary": "DeepSeek released V4.",
                    "content": "",
                    "site_name": "Example News",
                    "date": "2026-06-01T00:00:00+00:00",
                    "authority": 3
                },
                { "title": "Skip", "url": "ftp://example.com/nope" },
                { "title": "Dup", "url": "https://example.com/deepseek-v4/" }
            ]
        });
        let result = parse_deepseek_search_response("q", "pro_max", &body).unwrap();
        assert!(result.ok);
        assert_eq!(result.model, "deepseek-search");
        assert_eq!(result.sources.len(), 1);
        assert_eq!(result.sources[0].title, "DeepSeek V4");
        assert_eq!(result.sources[0].url, "https://example.com/deepseek-v4");
        assert_eq!(result.sources[0].site_name.as_deref(), Some("Example News"));
        assert!(result.answer.contains("https://example.com/deepseek-v4"));
        assert!(result.answer.contains("DeepSeek released V4."));
        assert_eq!(result.search_count, 1);
        assert_eq!(result.usage.total_tokens, 0);
    }

    #[test]
    fn empty_results_are_ok() {
        let result =
            parse_deepseek_search_response("q", "pro_max", &json!({ "results": [] })).unwrap();
        assert!(result.ok);
        assert!(result.sources.is_empty());
        assert!(result.answer.is_empty());
        assert_eq!(result.search_count, 0);
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
            .and(path("/v1/tools/search"))
            .and(body_partial_json(json!({
                "query": "latest stable Rust edition",
                "count": 10,
                "need_content": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{
                    "title": "Edition Guide",
                    "url": "https://doc.rust-lang.org/edition-guide/",
                    "summary": "The 2024 edition is stable.",
                    "site_name": "Rust"
                }]
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
        assert_eq!(result.model, "deepseek-search");
        assert!(result.answer.contains("The 2024 edition is stable."));
        assert_eq!(result.sources.len(), 1);
        assert_eq!(result.sources[0].title, "Edition Guide");
        assert_eq!(
            result.sources[0].url,
            "https://doc.rust-lang.org/edition-guide/"
        );
    }

    #[tokio::test]
    async fn http_error_includes_status() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/tools/search"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "message": "invalid api key",
                    "type": "invalid_request_error",
                    "param": null,
                    "code": "invalid_request_error"
                }
            })))
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
        assert!(msg.contains("invalid api key"), "{msg}");
    }
}
