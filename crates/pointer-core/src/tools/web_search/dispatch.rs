//! Single entry for chat-runtime `web_search` tool execution.

use crate::agent_instance_scope::AgentInstanceScope;
use crate::chat_service::StreamTx;
use crate::llm_token_stats::{ChatLlmTokenSession, ConversationLlmStats};
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

pub(crate) enum WebSearchTokenSink<'a> {
    Lead(&'a mut ChatLlmTokenSession),
    Sub {
        stats: &'a mut ConversationLlmStats,
        scope: &'a AgentInstanceScope,
    },
}

impl<'a> WebSearchTokenSink<'a> {
    fn into_recorder(self) -> WebSearchTokenRecorder<'a> {
        match self {
            WebSearchTokenSink::Lead(s) => WebSearchTokenRecorder::Lead(s),
            WebSearchTokenSink::Sub { stats, scope } => WebSearchTokenRecorder::Sub { stats, scope },
        }
    }
}

pub(crate) struct WebSearchDispatchContext<'a> {
    pub settings: &'a ModelSettings,
    pub agent_id: Option<&'a str>,
    pub args: Value,
    pub cancel: CancellationToken,
    pub stream: StreamTx,
    pub message_id: String,
    pub tool_call_id: String,
    pub invoke: WebSearchInvokeContext<'a>,
    pub token_sink: WebSearchTokenSink<'a>,
    pub trace_id: Option<String>,
}

/// Execute `web_search` (Tool or ResearchSubAgent mode) and record tokens.
pub(crate) async fn dispatch(ctx: WebSearchDispatchContext<'_>) -> Result<WebSearchResult> {
    let mut token_recorder = ctx.token_sink.into_recorder();
    let ui = WebSearchStreamUi {
        stream: ctx.stream.clone(),
        message_id: ctx.message_id.clone(),
        tool_call_id: ctx.tool_call_id.clone(),
        trace_id: ctx.trace_id.clone(),
    };

    let result = match &ctx.invoke {
        WebSearchInvokeContext::Tool => {
            tool_mode::execute(
                ctx.settings,
                ctx.agent_id,
                &ctx.args,
                ctx.cancel,
                ui,
            )
            .await
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
            let body = serde_json::to_string(&result)
                .map_err(|e| anyhow!("failed to serialize web search result: {e}"))?;
            Ok((body, true, None))
        }
        Err(e) => {
            let msg = format!("{e:#}");
            Ok((web_search_error_json(&query, &msg), false, Some(msg)))
        }
    }
}
