//! Lead single-agent wrapper over shared `agent_tool_pass`.

use anyhow::Result;

use super::agent_tool_pass::{
    run_agent_tool_pass, LeadSingleToolPassRequest, LeadToolPassConfig, ToolInvocationStats,
    ToolPassContext, ToolPassRequest, ToolPassResult as InnerToolPassResult,
};
use super::context::{SessionRefs, TranscriptPersist, TranscriptRefs};
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
    let stream = req.session.stream.clone();
    let stream_for_trim = stream.clone();
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
    let session = SessionRefs {
        stream: &stream,
        state: req.session.state,
        conversation_id: req.session.conversation_id,
        cancel: req.session.cancel,
    };
    let work_items_enabled = req.settings.task_board_work_items_enabled;
    let b42_enforced = work_items_enabled
        && req.file_tool_lead_for_invoke == crate::agents::AgentProfile::Computer;
    let computer_no_exec_init = req.settings.task_board_computer_no_exec_init
        && req.settings.task_board_planner_enabled
        && req.file_tool_lead_for_invoke == crate::agents::AgentProfile::Computer;
    let pass = ToolPassRequest {
        ctx: ToolPassContext {
            session,
            transcript: TranscriptRefs {
                history: req.history,
            },
            persist: TranscriptPersist::Main,
            message_id: req.assistant_id,
            task_board_store_key: req.main_task_board_store_key,
            tool_approval_mode: req.tool_approval_mode,
            tool_budget: req.tool_budget,
            consumed_single: Some(req.consumed_single),
            provider: req.provider,
            stats: &mut stats,
            lead: Some(LeadToolPassConfig {
                run_id: &run_id,
                allow_agents: req.allow_agents,
                enabled_skill_ids: req.enabled_skill_ids,
                agent_trace: req.agent_trace,
                file_tool_lead_for_invoke: req.file_tool_lead_for_invoke,
                lead_agent_id: req.lead_agent_id,
            }),
            sub: None,
            task_board_work_items_enabled: work_items_enabled,
            task_board_b42_enforced: b42_enforced,
            task_board_computer_no_exec_init: computer_no_exec_init,
            workspace_root: &req.settings.workspace_root,
        },
        final_tool_calls: req.final_tool_calls,
        trim_hook: Some(trim_hook),
        cancel: req.cancel,
    };
    match run_agent_tool_pass(pass).await? {
        InnerToolPassResult::SubFinished(_) => Ok(ToolPassResult::Finished),
        InnerToolPassResult::NoopExit => Ok(ToolPassResult::NoopExit),
        InnerToolPassResult::FinalReplyComplete(output) => {
            Ok(ToolPassResult::FinalReplyComplete(output))
        }
        InnerToolPassResult::RanTools => Ok(ToolPassResult::RanTools),
    }
}
