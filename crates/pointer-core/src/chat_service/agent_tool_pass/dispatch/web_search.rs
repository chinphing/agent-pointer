//! Web search async tool dispatch.

use crate::models::ToolCall;
use crate::provider::OpenAIProvider;
use crate::tools::web_search::{
    dispatch_to_tool_json_async, WebSearchDispatchContext, WebSearchInvokeContext, WebSearchTokenSink,
};
use anyhow::anyhow;
use tokio_util::sync::CancellationToken;

use super::super::super::StreamTx;
use super::super::types::{SubToolPassConfig, ToolExecResult, ToolInvocationStats};

pub(super) async fn dispatch_web_search(
    stream: &StreamTx,
    provider: &OpenAIProvider,
    message_id: &str,
    history: &[crate::models::ChatMessage],
    tc: &ToolCall,
    args_value: serde_json::Value,
    cancel: &CancellationToken,
    stats: &mut ToolInvocationStats<'_>,
    lead_agent_id: Option<&str>,
    sub: Option<&mut SubToolPassConfig<'_>>,
) -> ToolExecResult {
    let invoke = if sub.as_ref().map(|s| s.def.id.as_str()) == Some("research") {
        WebSearchInvokeContext::ResearchSubAgent {
            history,
            exclude_message_id: message_id,
        }
    } else {
        WebSearchInvokeContext::Tool
    };
    let token_sink = match stats {
        ToolInvocationStats::TokenSession(s) => WebSearchTokenSink::Lead(s),
        ToolInvocationStats::Conversation(s) => WebSearchTokenSink::Sub {
            stats: s,
            scope: &sub
                .as_ref()
                .ok_or_else(|| anyhow!("web_search sub scope missing"))?
                .instance_scope,
        },
    };
    let agent_id = sub.as_ref().map(|s| s.def.id.as_str()).or(lead_agent_id);
    let trace_id = sub.as_ref().map(|s| s.trace_id.clone());
    dispatch_to_tool_json_async(WebSearchDispatchContext {
        settings: &provider.settings,
        agent_id,
        args: args_value,
        cancel: cancel.clone(),
        stream: stream.clone(),
        message_id: message_id.to_string(),
        tool_call_id: tc.id.clone(),
        history,
        exclude_message_id: message_id,
        invoke,
        token_sink,
        trace_id,
    })
    .await
}
