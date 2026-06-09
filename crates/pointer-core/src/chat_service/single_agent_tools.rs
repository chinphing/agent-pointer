//! Lead single-agent wrapper over shared `agent_tool_pass`.

use crate::agents::AgentProfile;
use crate::llm_token_stats::ChatLlmTokenSession;
use crate::models::{AgentTrace, ChatMessage, ModelSettings, ToolCall};
use crate::task_board::TaskBoardTrimHook;
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
    main_task_board_store_key: &str,
    history: &mut Vec<ChatMessage>,
    allow_agents: &[String],
    enabled_skill_ids: &mut Vec<String>,
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
    settings: &ModelSettings,
    lead_agent_id: &str,
) -> Result<ToolPassResult> {
    let _ = reasoning_in_messages;
    let run_id = llm_token_session.run_id.clone();
    let mut stats = ToolInvocationStats::TokenSession(llm_token_session);
    let stream_for_trim = stream.clone();
    let anchor_message_id =
        state.get_main_task_board_anchor(conversation_id, main_task_board_store_key);
    let trim_hook = TaskBoardTrimHook {
        settings,
        agent_id: lead_agent_id,
        conversation_id,
        stream: &stream_for_trim,
        emit_history_replaced: true,
        anchor_message_id: anchor_message_id.as_deref(),
    };
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
        main_task_board_store_key,
        &mut stats,
        final_tool_calls,
        Some(LeadToolPassConfig {
            run_id: &run_id,
            allow_agents,
            enabled_skill_ids,
            agent_trace,
            raw_content_buf,
            file_tool_lead_for_invoke,
            lead_agent_id,
        }),
        None,
        Some(trim_hook),
    )
    .await?
    {
        InnerToolPassResult::SubFinished(_) => Ok(ToolPassResult::Finished),
        InnerToolPassResult::NoopExit => Ok(ToolPassResult::NoopExit),
        InnerToolPassResult::RanTools => Ok(ToolPassResult::RanTools),
    }
}
