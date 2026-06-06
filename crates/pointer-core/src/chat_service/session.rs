//! Session entry: cancel registration, `run_chat_inner`, `Done` / error streaming.

use crate::models::{ChatMessage, StreamEvent};
use anyhow::Result;
use std::backtrace::Backtrace;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use uuid::Uuid;

use super::app_state::AppState;
use super::emit::emit;
use super::StreamTx;

pub async fn run_chat(
    stream: StreamTx,
    state: Arc<AppState>,
    conversation_id: String,
    mut history: Vec<ChatMessage>,
    mut enabled_skill_ids: Vec<String>,
    agent_mode: Option<String>,
    tool_rounds_used_single_start: u32,
    tool_rounds_used_supervisor_start: u32,
    workspace_root: String,
) -> Result<()> {
    log::info!(
        "run_chat start conversation_id={} incoming_history_messages={} enabled_skill_ids={} request_agent_mode={:?} tool_rounds_used_single_start={} tool_rounds_used_supervisor_start={}",
        conversation_id,
        history.len(),
        enabled_skill_ids.len(),
        agent_mode,
        tool_rounds_used_single_start,
        tool_rounds_used_supervisor_start,
    );

    let cancel = CancellationToken::new();
    state
        .cancels
        .lock()
        .insert(conversation_id.clone(), cancel.clone());

    let run_id = Uuid::new_v4().to_string();
    let mut consumed_single = 0u32;
    let mut consumed_supervisor = 0u32;
    let result = super::session_inner::run_chat_inner(
        stream.clone(),
        state.clone(),
        &conversation_id,
        &mut history,
        &mut enabled_skill_ids,
        agent_mode.as_deref(),
        tool_rounds_used_single_start,
        tool_rounds_used_supervisor_start,
        workspace_root,
        &mut consumed_single,
        &mut consumed_supervisor,
        cancel.clone(),
        &run_id,
    )
    .await;

    if let Err(e) = crate::token_usage_store::finalize_run(&run_id, &conversation_id, &history) {
        log::warn!(
            "token_usage_store: finalize_run failed run_id={run_id} conversation_id={conversation_id}: {e}"
        );
    }
    if let Err(e) =
        crate::token_usage_store::flush_unsent_reports(&state.platform_auth).await
    {
        log::warn!("token_usage_store: flush after chat failed: {e}");
    }

    state.cancels.lock().remove(&conversation_id);

    if let Err(err) = &result {
        // Root cause is in `err` (often an HTTP/API message). `Backtrace::capture()` here only
        // shows the async poll point (e.g. chat_service + tokio), not the failing await site.
        log::error!("run_chat failed conversation_id={} error={:#}", conversation_id, err);
        log::debug!(
            "run_chat failure poll-point backtrace (for deep debugging):\n{}",
            Backtrace::capture()
        );
        emit(
            &stream,
            StreamEvent::Error {
                message_id: None,
                message: err.to_string(),
            },
        );
        log::info!(
            "run_chat emitted StreamEvent::Error (session-level) conversation_id={} message_len_chars={}",
            conversation_id,
            err.to_string().chars().count(),
        );
    }
    let max_tr = state
        .effective_settings()
        .max_tool_rounds;
    let single_total = tool_rounds_used_single_start.saturating_add(consumed_single);
    let supervisor_total =
        tool_rounds_used_supervisor_start.saturating_add(consumed_supervisor);
    log::info!(
        "run_chat conversation end: emitting StreamEvent::Done conversation_id={} run_outcome={} history_messages_final={} consumed_this_run_single={} consumed_this_run_supervisor={} cumulative_tool_rounds_single={} cumulative_tool_rounds_supervisor={} max_tool_rounds_attached={}",
        conversation_id,
        if result.is_ok() { "Ok" } else { "Err" },
        history.len(),
        consumed_single,
        consumed_supervisor,
        single_total,
        supervisor_total,
        max_tr,
    );
    let main_store_key = state
        .get_active_main_task_board_key(&conversation_id)
        .unwrap_or_else(|| conversation_id.clone());
    if result.is_ok()
        && crate::task_board::maybe_auto_finalize_if_complete(&state.task_board_store, &main_store_key)
    {
        let doc = state.task_board_store.document(&main_store_key);
        let anchor_message_id = state.get_main_task_board_anchor(&conversation_id, &main_store_key);
        super::emit::emit_task_board_updated(
            &stream,
            &conversation_id,
            &main_store_key,
            anchor_message_id,
            doc.to_value(),
        );
    }
    let done_conversation_id = conversation_id.clone();
    if stream
        .send(StreamEvent::Done {
            conversation_id,
            tool_rounds_used_total: Some(single_total),
            tool_rounds_used_supervisor_total: Some(supervisor_total),
            max_tool_rounds: Some(max_tr),
        })
        .is_err()
    {
        log::warn!(
            "run_chat: StreamEvent::Done not delivered (stream receiver dropped) conversation_id={}",
            done_conversation_id
        );
    }
    log::info!(
        "run_chat finished after Done emit final_result_is_ok={}",
        result.is_ok(),
    );
    result
}
