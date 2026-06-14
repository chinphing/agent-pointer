//! Shared per-round phases for lead single-agent and sub-agent tool loops.
//!
//! **Differences kept at call sites (not unified here):**
//! - **Prompt prep**: `prepare_single_agent_round_prompts` vs `prepare_sub_agent_round_prompts`
//! - **Stream**: `run_provider_stream_round` vs `run_sub_agent_stream_round` (sub uses parent `message_id`, trace UI)
//! - **History**: main `history` + DB persist vs sub `local_history` only (`persist_transcript = sub.is_none()` in tool pass)
//! - **Tool pass**: `run_single_agent_tool_pass` vs `run_agent_tool_pass` + `SubToolPassConfig`
//! - **Supervisor**: no inner loop; delegates each task to `run_sub_agent`

use anyhow::{anyhow, Result};

use crate::agents::AgentProfile;
use crate::models::ToolCall;
use crate::tools::ToolRegistry;

use super::agent_post_stream::{
    bail_on_tool_budget_exhausted, decide_when_no_tool_calls, decide_when_tool_calls_present,
    PostAssistantTurnAction,
};
use super::app_state::AppState;
use super::context::PostAssistantContext;
use super::session_budget::SessionToolBudget;
use tokio_util::sync::CancellationToken;

/// Loop guard: cancel token or zero remaining tool budget.
pub(super) enum LoopGuardOutcome {
    Continue,
    Cancelled,
    BudgetExhausted,
}

pub(super) fn check_loop_guards(
    cancel: &CancellationToken,
    tool_budget: &SessionToolBudget,
) -> LoopGuardOutcome {
    if cancel.is_cancelled() {
        LoopGuardOutcome::Cancelled
    } else if tool_budget.remaining() == 0 {
        LoopGuardOutcome::BudgetExhausted
    } else {
        LoopGuardOutcome::Continue
    }
}

/// Computer agent: post-round hook and optional give-up stop.
pub(super) fn computer_round_complete_or_give_up(
    state: &AppState,
    conversation_id: &str,
    profile: AgentProfile,
    thoughts: Option<&str>,
    tool_calls: Option<&[ToolCall]>,
    mark_cancelled_on_give_up: bool,
) -> Result<()> {
    if profile != AgentProfile::Computer {
        return Ok(());
    }
    state
        .computer_state
        .on_assistant_round_complete(conversation_id, thoughts, tool_calls);
    if !state.computer_state.should_give_up(conversation_id) {
        return Ok(());
    }
    if mark_cancelled_on_give_up {
        state.computer_state.mark_cancelled(conversation_id);
    }
    Err(anyhow!(
        "当前任务已尽力但仍无法完成（重复操作达到 {} 次），请提供进一步指导。",
        crate::agents::computer::tier::GIVE_UP_THRESHOLD
    ))
}

/// After assistant turn persisted: format retry vs tool execution vs successful stop.
pub(super) async fn resolve_post_assistant_action(
    ctx: &mut PostAssistantContext<'_>,
    tools: &ToolRegistry,
    final_tool_calls: &[ToolCall],
    log_prefix: &str,
) -> Result<PostAssistantTurnAction> {
    if final_tool_calls.is_empty() {
        decide_when_no_tool_calls(ctx).await
    } else {
        decide_when_tool_calls_present(tools, final_tool_calls, log_prefix).await
    }
}

/// Record one tool cycle, sync counters, and fail if budget is exhausted.
pub(super) async fn finish_tool_round_cycle(ctx: &mut PostAssistantContext<'_>) -> Result<()> {
    ctx.budget.tool_budget.record_tool_cycle();
    if let Some(consumed) = ctx.budget.consumed_single.as_mut() {
        ctx.budget.tool_budget.sync_out(consumed);
    }
    bail_on_tool_budget_exhausted(ctx).await
}
