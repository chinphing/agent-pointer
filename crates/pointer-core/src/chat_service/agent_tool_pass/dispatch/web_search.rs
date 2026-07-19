//! Web search async tool dispatch.

use crate::agent_instance_scope::AgentInstanceScope;
use crate::models::ToolCall;
use crate::provider::OpenAIProvider;
use crate::tools::web_search::{
    dispatch_to_tool_json_async, WebSearchDispatchContext, WebSearchInvokeContext,
};
use anyhow::{anyhow, Result};
use tokio_util::sync::CancellationToken;

use super::super::super::StreamTx;
use super::super::types::{SubToolPassConfig, ToolExecResult};

#[derive(Clone)]
pub(in crate::chat_service::agent_tool_pass) struct WebSearchInvocation {
    usage_scope: AgentInstanceScope,
    agent_id: Option<String>,
    research_sub_agent: bool,
    trace_id: Option<String>,
    scoped_message_id: Option<String>,
}

pub(super) fn resolve_web_search_usage_scope(
    lead_scope: Option<&AgentInstanceScope>,
    sub_scope: Option<&AgentInstanceScope>,
) -> Result<AgentInstanceScope> {
    sub_scope
        .or(lead_scope)
        .cloned()
        .ok_or_else(|| anyhow!("web_search usage scope missing"))
}

pub(in crate::chat_service::agent_tool_pass) fn prepare_web_search_invocation(
    lead_scope: Option<&AgentInstanceScope>,
    lead_agent_id: Option<&str>,
    sub: Option<&SubToolPassConfig<'_>>,
) -> Result<WebSearchInvocation> {
    Ok(WebSearchInvocation {
        usage_scope: resolve_web_search_usage_scope(
            lead_scope,
            sub.map(|s| s.instance_scope),
        )?,
        agent_id: sub
            .map(|s| s.active.def.id.clone())
            .or_else(|| lead_agent_id.map(str::to_string)),
        research_sub_agent: sub.map(|s| s.active.def.id.as_str()) == Some("research"),
        trace_id: sub.map(|s| s.trace_id.clone()),
        scoped_message_id: sub.map(|s| s.scoped_message_id.clone()),
    })
}

pub(super) async fn dispatch_web_search(
    stream: &StreamTx,
    provider: &OpenAIProvider,
    message_id: &str,
    history: &[crate::models::ChatMessage],
    tc: &ToolCall,
    args_value: serde_json::Value,
    cancel: &CancellationToken,
    invocation: WebSearchInvocation,
) -> ToolExecResult {
    let invoke = if invocation.research_sub_agent {
        WebSearchInvokeContext::ResearchSubAgent {
            history,
            exclude_message_id: message_id,
        }
    } else {
        WebSearchInvokeContext::Tool
    };
    dispatch_to_tool_json_async(WebSearchDispatchContext {
        settings: &provider.settings,
        agent_id: invocation.agent_id.as_deref(),
        args: args_value,
        cancel: cancel.clone(),
        stream: stream.clone(),
        message_id: message_id.to_string(),
        tool_call_id: tc.id.clone(),
        history,
        exclude_message_id: message_id,
        invoke,
        usage_scope: invocation.usage_scope,
        trace_id: invocation.trace_id,
        scoped_message_id: invocation.scoped_message_id,
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_instance_scope::AgentInstanceScope;

    #[test]
    fn resolves_owned_usage_scope_for_lead_and_sub_dispatch() {
        let lead = AgentInstanceScope::with_instance_id(
            "run-lead",
            "conversation",
            "general",
            "lead-instance",
        );
        let sub = AgentInstanceScope::with_instance_id(
            "run-sub",
            "conversation",
            "research",
            "sub-instance",
        );

        let lead_resolved =
            resolve_web_search_usage_scope(Some(&lead), None).expect("lead scope");
        assert_eq!(lead_resolved.agent_instance_id, "lead-instance");

        let sub_resolved =
            resolve_web_search_usage_scope(None, Some(&sub)).expect("sub scope");
        assert_eq!(sub_resolved.agent_instance_id, "sub-instance");
    }

    #[test]
    fn rejects_dispatch_without_usage_scope() {
        let err = resolve_web_search_usage_scope(None, None).expect_err("missing scope");
        assert!(err.to_string().contains("web_search usage scope missing"));
    }
}
