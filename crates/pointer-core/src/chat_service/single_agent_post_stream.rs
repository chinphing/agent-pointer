//! Lead single-agent wrappers over shared `agent_post_stream`.

pub(super) use super::agent_post_stream::PostAssistantTurnAction;

use crate::agents::AgentPlan;
use crate::models::{AgentTrace, ChatMessage, ModelSettings, ToolCall};
use crate::provider::OpenAIProvider;
use crate::tools::ToolRegistry;
use anyhow::Result;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::app_state::AppState;
use super::session_budget::SessionToolBudget;
use super::StreamTx;

pub(super) fn build_assistant_message_after_stream(
    assistant_id: &str,
    raw_content_buf: &str,
    reasoning_buf: String,
    reasoning_in_messages: bool,
    final_tool_calls: &[ToolCall],
    xml_thoughts: Option<String>,
    agent_plan: &AgentPlan,
    agent_instance_id: Option<String>,
    agent_trace: &[AgentTrace],
    state: &AppState,
) -> ChatMessage {
    super::agent_post_stream::build_lead_assistant_message_after_stream(
        assistant_id,
        raw_content_buf,
        reasoning_buf,
        reasoning_in_messages,
        final_tool_calls,
        xml_thoughts,
        agent_plan,
        agent_instance_id,
        agent_trace,
        state,
    )
}

pub(super) fn commit_assistant_turn(
    stream: &StreamTx,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    assistant_id: &str,
    assistant_msg: &ChatMessage,
) {
    super::agent_post_stream::commit_lead_assistant_turn(
        stream,
        conversation_id,
        history,
        assistant_id,
        assistant_msg,
    );
}

pub(super) async fn decide_when_no_tool_calls(
    stream: &StreamTx,
    state: &Arc<AppState>,
    history: &mut Vec<ChatMessage>,
    settings: &ModelSettings,
    provider: &OpenAIProvider,
    conversation_id: &str,
    cancel: &CancellationToken,
    tool_budget: &mut SessionToolBudget,
    consumed_single: &mut u32,
    max_cap: u32,
    compression_scope: crate::agent_instance_scope::AgentInstanceScope,
) -> Result<PostAssistantTurnAction> {
    super::agent_post_stream::decide_when_no_tool_calls(
        stream,
        state.as_ref(),
        history,
        settings,
        provider,
        conversation_id,
        cancel,
        tool_budget,
        Some(consumed_single),
        max_cap,
        &super::agent_post_stream::ToolBudgetExhaustionScope::lead_single(
            max_cap,
            compression_scope,
        ),
    )
    .await
}

pub(super) async fn decide_when_tool_calls_present(
    tools: &ToolRegistry,
    final_tool_calls: &[ToolCall],
) -> Result<PostAssistantTurnAction> {
    super::agent_post_stream::decide_when_tool_calls_present(
        tools,
        final_tool_calls,
        "lead",
    )
    .await
}

pub(super) async fn bail_on_tool_budget_exhausted(
    stream: &StreamTx,
    state: &Arc<AppState>,
    history: &mut Vec<ChatMessage>,
    settings: &ModelSettings,
    provider: &OpenAIProvider,
    conversation_id: &str,
    cancel: &CancellationToken,
    tool_budget: &mut SessionToolBudget,
    consumed_single: &mut u32,
    max_cap: u32,
    compression_scope: crate::agent_instance_scope::AgentInstanceScope,
) -> Result<()> {
    super::agent_post_stream::bail_on_tool_budget_exhausted(
        stream,
        state.as_ref(),
        history,
        settings,
        provider,
        conversation_id,
        cancel,
        tool_budget,
        Some(consumed_single),
        max_cap,
        &super::agent_post_stream::ToolBudgetExhaustionScope::lead_single(
            max_cap,
            compression_scope,
        ),
    )
    .await
}
