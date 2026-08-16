//! Single entry for chat-runtime `web_search` tool execution.

use crate::agent_instance_scope::AgentInstanceScope;
use crate::chat_service::StreamTx;
use crate::models::{ChatMessage, ModelSettings};
use anyhow::{anyhow, Result};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::client::WebSearchResult;
use super::stream_ui::WebSearchStreamUi;
use super::token::WebSearchTokenRecorder;
use super::tool_mode;
use super::web_search_error_json;

/// How to build DashScope `input.messages` for this invocation.
#[derive(Debug, Clone)]
pub(crate) enum WebSearchInvokeContext<'a> {
    /// Generic tool: single user message = `query` only.
    Tool,
    /// Research sub-agent: AGENT.md system + local history + query.
    ResearchSubAgent {
        history: &'a [ChatMessage],
        exclude_message_id: &'a str,
    },
}

pub(crate) struct WebSearchDispatchContext<'a> {
    pub settings: &'a ModelSettings,
    pub agent_id: Option<&'a str>,
    pub args: Value,
    pub cancel: CancellationToken,
    pub stream: StreamTx,
    pub message_id: String,
    pub tool_call_id: String,
    pub history: &'a [ChatMessage],
    pub exclude_message_id: &'a str,
    pub invoke: WebSearchInvokeContext<'a>,
    pub usage_scope: AgentInstanceScope,
    pub trace_id: Option<String>,
    pub scoped_message_id: Option<String>,
}

/// Execute `web_search` (Tool or ResearchSubAgent mode) and record tokens.
pub(crate) async fn dispatch(ctx: WebSearchDispatchContext<'_>) -> Result<WebSearchResult> {
    let citation_base_index =
        super::client::compute_citation_base_index(ctx.history, ctx.exclude_message_id);
    let token_recorder = WebSearchTokenRecorder::new(
        ctx.usage_scope,
        crate::llm_token_stats::active_provider_source(ctx.settings),
    );
    let ui = WebSearchStreamUi {
        stream: ctx.stream.clone(),
        message_id: ctx.message_id.clone(),
        tool_call_id: ctx.tool_call_id.clone(),
        trace_id: ctx.trace_id.clone(),
        scoped_message_id: ctx.scoped_message_id.clone(),
        citation_base_index,
    };

    let result = match &ctx.invoke {
        WebSearchInvokeContext::Tool => {
            tool_mode::execute(ctx.settings, ctx.agent_id, &ctx.args, ctx.cancel, ui).await
        }
        WebSearchInvokeContext::ResearchSubAgent {
            history,
            exclude_message_id,
        } => {
            crate::agents::research::web_search::execute(
                ctx.settings,
                ctx.agent_id,
                history,
                exclude_message_id,
                &ctx.args,
                ctx.cancel,
                ui,
            )
            .await
        }
    }?;

    let result = super::client::apply_citation_base_index(result, citation_base_index);
    let result = super::client::finalize_web_search_result(result);
    token_recorder.record(&result);
    Ok(result)
}

pub(crate) async fn dispatch_to_tool_json_async(
    ctx: WebSearchDispatchContext<'_>,
) -> Result<(String, bool, Option<String>)> {
    let query = ctx
        .args
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    match dispatch(ctx).await {
        Ok(result) => {
            let mut value = serde_json::to_value(&result)
                .map_err(|e| anyhow!("failed to serialize web search result: {e}"))?;
            let citation_md = super::client::format_sources_citation_markdown(&result.sources);
            let for_reply = super::client::format_sources_for_reply(&result.sources);
            if !for_reply.is_empty() {
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("sourcesForReply".into(), Value::String(for_reply));
                    obj.insert("sourcesCitationMarkdown".into(), Value::String(citation_md));
                    obj.insert(
                        "citationGuide".into(),
                        Value::String(
                            "Paste sourcesForReply verbatim. N. [title](url) matches [N] in answer. \
                             Multi-search in the same user turn: citationBaseIndex offsets each call \
                             so [N] stays unique — concatenate sourcesForReply from all calls."
                                .into(),
                        ),
                    );
                    obj.insert(
                        "multiSearchGuide".into(),
                        Value::String(
                            "Same user turn: each call has citationBaseIndex (prior max source index). \
                             [N] and sources[].index are shifted globally — safe to cite [N] across calls. \
                             Append all sourcesForReply blocks under one ## Sources (dedupe URLs if needed)."
                                .into(),
                        ),
                    );
                }
            }
            let body = serde_json::to_string(&value)
                .map_err(|e| anyhow!("failed to serialize web search result: {e}"))?;
            Ok((body, true, None))
        }
        Err(e) => {
            let msg = format!("{e:#}");
            Ok((web_search_error_json(&query, &msg), false, Some(msg)))
        }
    }
}
