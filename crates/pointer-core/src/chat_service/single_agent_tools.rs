//! Lead single-agent wrapper over shared `agent_tool_pass`.

use anyhow::Result;

use super::agent_tool_pass::{
    run_agent_tool_pass, LeadToolPassConfig, ToolInvocationStats, ToolPassResult as InnerToolPassResult,
};
use super::context::LeadSingleToolPassRequest;
use crate::task_board::TaskBoardTrimHook;

/// Outcome of executing a non-empty validated tool batch for one assistant turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ToolPassResult {
    Finished,
    NoopExit,
    /// Lone successful `final_reply` tool — caller should emit the delivery message.
    FinalReplyComplete(String),
    RanTools,
}

pub(super) async fn run_single_agent_tool_pass(
    req: LeadSingleToolPassRequest<'_>,
) -> Result<ToolPassResult> {
    let run_id = req.token_session.run_id.clone();
    let mut stats = ToolInvocationStats::TokenSession(req.token_session);
    let stream_for_trim = req.session.stream.clone();
    let anchor_message_id = req.session.state.get_main_task_board_anchor(
        req.session.conversation_id,
        req.main_task_board_store_key,
    );
    let trim_hook = TaskBoardTrimHook {
        settings: req.settings,
        agent_id: req.lead_agent_id,
        conversation_id: req.session.conversation_id,
        stream: &stream_for_trim,
        emit_trim_ui_event: true,
        anchor_message_id: anchor_message_id.as_deref(),
    };
    match run_agent_tool_pass(
        req.session.stream.clone(),
        req.session.state,
        req.session.conversation_id,
        req.assistant_id,
        req.history,
        req.tool_approval_mode,
        req.tool_budget,
        Some(req.consumed_single),
        req.cancel,
        req.provider,
        req.main_task_board_store_key,
        &mut stats,
        req.final_tool_calls,
        Some(LeadToolPassConfig {
            run_id: &run_id,
            allow_agents: req.allow_agents,
            enabled_skill_ids: req.enabled_skill_ids,
            agent_trace: req.agent_trace,
            file_tool_lead_for_invoke: req.file_tool_lead_for_invoke,
            lead_agent_id: req.lead_agent_id,
        }),
        None,
        Some(trim_hook),
    )
    .await?
    {
        InnerToolPassResult::SubFinished(_) => Ok(ToolPassResult::Finished),
        InnerToolPassResult::NoopExit => Ok(ToolPassResult::NoopExit),
        InnerToolPassResult::FinalReplyComplete(output) => {
            Ok(ToolPassResult::FinalReplyComplete(output))
        }
        InnerToolPassResult::RanTools => Ok(ToolPassResult::RanTools),
    }
}
