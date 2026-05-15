//! Lead single-agent wrapper over shared `agent_tool_pass`.

use crate::agents::AgentProfile;
use crate::llm_token_stats::ChatLlmTokenSession;
use crate::models::{AgentTrace, ChatMessage, ToolCall};
use crate::provider::OpenAIProvider;
use anyhow::Result;
use tokio_util::sync::CancellationToken;

use super::agent_tool_pass::{
    run_agent_tool_pass, LeadToolPassConfig, ToolInvocationStats, ToolPassResult as InnerToolPassResult,
};
use super::app_state::AppState;
use super::session_budget::SessionToolBudget;
use super::StreamTx;

/// Outcome of executing a non-empty validated tool batch for one assistant turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolPassResult {
    Finished,
    NoopExit,
    RanTools,
}

pub(super) async fn run_single_agent_tool_pass(
    stream: StreamTx,
    state: &AppState,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    enabled_skill_ids: &[String],
    provider: &OpenAIProvider,
    tool_approval_mode: &str,
    tool_budget: &mut SessionToolBudget,
    consumed_single: &mut u32,
    cancel: CancellationToken,
    llm_token_session: &mut ChatLlmTokenSession,
    reasoning_in_messages: bool,
    assistant_id: String,
    file_tool_lead_for_invoke: AgentProfile,
    final_tool_calls: &[ToolCall],
    raw_content_buf: &str,
    agent_trace: &mut Vec<AgentTrace>,
) -> Result<ToolPassResult> {
    let _ = reasoning_in_messages;
    let mut stats = ToolInvocationStats::TokenSession(llm_token_session);
    match run_agent_tool_pass(
        stream,
        state,
        conversation_id,
        assistant_id,
        history,
        tool_approval_mode,
        tool_budget,
        Some(consumed_single),
        cancel,
        provider,
        conversation_id,
        &mut stats,
        final_tool_calls,
        Some(LeadToolPassConfig {
            enabled_skill_ids,
            agent_trace,
            raw_content_buf,
            file_tool_lead_for_invoke,
        }),
        None,
    )
    .await?
    {
        InnerToolPassResult::LeadFinished => Ok(ToolPassResult::Finished),
        InnerToolPassResult::SubFinished(_) => Ok(ToolPassResult::Finished),
        InnerToolPassResult::NoopExit => Ok(ToolPassResult::NoopExit),
        InnerToolPassResult::RanTools => Ok(ToolPassResult::RanTools),
    }
}
