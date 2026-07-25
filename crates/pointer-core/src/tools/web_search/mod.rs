mod client;
mod dispatch;
mod generation;
mod responses;
mod sse_drain;
mod stream_ui;
mod token;
mod tool_mode;

pub use client::{
    apply_citation_base_index, build_search_request_body, build_search_stream_request_body,
    build_tool_generation_request_body, build_tool_generation_stream_request_body,
    compute_citation_base_index, dashscope_model_uses_multimodal_endpoint,
    dashscope_native_generation_url, execute_web_search, execute_web_search_stream,
    finalize_web_search_result, format_merged_sources_for_reply, format_sources_citation_markdown,
    format_sources_for_reply, format_sources_title_list, history_to_dashscope_messages,
    normalize_search_strategy, parse_search_response, parse_search_sse_chunk,
    resolve_dashscope_search_config, resolve_web_search_api_model, resolve_web_search_citations,
    web_search_unsupported_on_generation_api, SearchSseAccumulator, SearchSseChunk,
    WebSearchMessage, WebSearchRequest, WebSearchResult, WebSearchSource,
    DEFAULT_TOOL_WEB_SEARCH_STRATEGY, DEFAULT_WEB_SEARCH_STRATEGY,
};
pub(crate) use dispatch::{
    dispatch_to_tool_json_async, WebSearchDispatchContext, WebSearchInvokeContext,
};
pub use generation::{execute_generation_web_search, DEFAULT_TOOL_WEB_SEARCH_MODEL};
pub use responses::{
    build_responses_request_body, dashscope_responses_url, execute_responses_web_search,
    parse_responses_response,
};
pub use sse_drain::{
    drain_search_sse_chunk, read_dashscope_search_sse, read_dashscope_search_sse_from_str,
    SearchStreamEvent,
};
pub use stream_ui::WebSearchStreamUi;

use super::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn register_all(reg: &ToolRegistry) {
    const DOC_SOURCE: &str = "tools/prompts/web_search.md";
    let doc = include_str!("../prompts/web_search.md").trim();
    let h: ToolHandler = Arc::new(|args| run_web_search_sync(args));
    reg.register(ToolEntry::new(
        "web_search",
        DOC_SOURCE,
        "medium",
        true,
        doc,
        h,
    ));
}

/// Default `search_options.search_strategy` for research sub-agent (Responses API metadata).
pub const DEFAULT_RESEARCH_WEB_SEARCH_STRATEGY: &str = "max";

fn parse_web_search_args_with_defaults(
    args: &Value,
    default_strategy: &str,
    default_enable_thinking: bool,
) -> Result<WebSearchRequest> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("missing or empty query"))?
        .to_string();

    let strategy_raw = args
        .get("searchStrategy")
        .or_else(|| args.get("search_strategy"))
        .and_then(|v| v.as_str())
        .unwrap_or(default_strategy);
    let search_strategy = normalize_search_strategy(strategy_raw)?.to_string();

    let enable_thinking = args
        .get("enableThinking")
        .or_else(|| args.get("enable_thinking"))
        .and_then(|v| v.as_bool())
        .unwrap_or(default_enable_thinking);

    let forced_search = args
        .get("forcedSearch")
        .or_else(|| args.get("forced_search"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let enable_vertical_search = args
        .get("enableVerticalSearch")
        .or_else(|| args.get("enable_vertical_search"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok(WebSearchRequest {
        query,
        search_strategy,
        forced_search,
        enable_vertical_search,
        enable_thinking,
        messages: Vec::new(),
    })
}

/// Parse tool args for generic `web_search` (Generation API, `pro_max` default).
pub fn parse_tool_web_search_args(args: &Value) -> Result<WebSearchRequest> {
    parse_web_search_args_with_defaults(args, DEFAULT_TOOL_WEB_SEARCH_STRATEGY, false)
}

/// Parse tool args for research sub-agent (Responses API, `max` + thinking default).
pub fn parse_research_web_search_args(args: &Value) -> Result<WebSearchRequest> {
    parse_web_search_args_with_defaults(args, DEFAULT_RESEARCH_WEB_SEARCH_STRATEGY, true)
}

/// Backward-compatible alias for generic tool parsing.
pub fn parse_web_search_args(args: &Value) -> Result<WebSearchRequest> {
    parse_tool_web_search_args(args)
}

/// Sync stub — real execution happens in `agent_tool_pass` async path.
fn run_web_search_sync(_args: Value) -> Result<String> {
    Err(anyhow!(
        "web_search is executed by the chat runtime async path, not synchronous invoke"
    ))
}

pub fn web_search_error_json(query: &str, message: &str) -> String {
    json!({
        "ok": false,
        "query": query,
        "error": message,
        "sources": [],
        "searchCount": 0,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_defaults() {
        let req = parse_tool_web_search_args(&json!({"query": "Rust 2024 edition"})).unwrap();
        assert_eq!(req.query, "Rust 2024 edition");
        assert_eq!(req.search_strategy, "pro_max");
        assert!(!req.enable_thinking);
        assert!(!req.forced_search);
    }

    #[test]
    fn parse_research_args_defaults() {
        let req = parse_research_web_search_args(&json!({"query": "Rust 2024 edition"})).unwrap();
        assert_eq!(req.search_strategy, "max");
        assert!(req.enable_thinking);
    }

    #[test]
    fn parse_args_snake_case_aliases() {
        let req = parse_web_search_args(&json!({
            "query": "weather",
            "search_strategy": "max",
            "forced_search": true,
            "enable_vertical_search": true
        }))
        .unwrap();
        assert_eq!(req.search_strategy, "max");
        assert!(req.forced_search);
        assert!(req.enable_vertical_search);
    }
}
