//! Session entry: cancel registration, `run_chat_inner`, `Done` / error streaming.

use crate::dispatcher::TriggerSource;
use crate::models::{ChatMessage, StreamEvent};
use anyhow::Result;
use std::backtrace::Backtrace;
use std::collections::HashMap;
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
    agent_skill_overrides: HashMap<String, Vec<String>>,
    agent_mode: Option<String>,
    lead_agent_id_override: Option<String>,
    tool_rounds_used_single_start: u32,
    tool_rounds_used_supervisor_start: u32,
    workspace_root: String,
    workspace_inherit_disabled: Option<bool>,
    trigger_source: Option<TriggerSource>,
) -> Result<()> {
    log::info!(
        "run_chat start conversation_id={} incoming_history_messages={} enabled_skill_ids={} request_agent_mode={:?} lead_agent_id_override={:?} tool_rounds_used_single_start={} tool_rounds_used_supervisor_start={}",
        conversation_id,
        history.len(),
        enabled_skill_ids.len(),
        agent_mode,
        lead_agent_id_override,
        tool_rounds_used_single_start,
        tool_rounds_used_supervisor_start,
    );

    super::sub_message::strip_scoped_from_lead_history(&mut history);

    state.touch_activity();

    let cancel = CancellationToken::new();
    state
        .cancels
        .lock()
        .insert(conversation_id.clone(), cancel.clone());

    let transcript_session = match crate::conversation_transcript::ConversationTranscriptSession::begin(
        &conversation_id,
        &mut history,
    ) {
        Ok(session) => Some(session),
        Err(e) => {
            log::warn!(
                "run_chat: transcript begin failed conversation_id={}: {e:#}",
                conversation_id
            );
            super::conversation_persist::append_missing(&conversation_id, &history);
            None
        }
    };

    let run_id = Uuid::new_v4().to_string();
    let mut consumed_single = 0u32;
    let mut consumed_supervisor = 0u32;
    let run_req = super::context::ChatRunRequest {
        agent_mode,
        lead_agent_id_override,
        agent_skill_overrides,
        tool_rounds_used_single_start,
        workspace_root,
        workspace_inherit_disabled,
        run_id: run_id.clone(),
        trigger_source,
    };
    let mut run_ctx = super::context::ChatRunContext {
        stream: stream.clone(),
        state: state.clone(),
        conversation_id: &conversation_id,
        history: &mut history,
        enabled_skill_ids: &mut enabled_skill_ids,
        consumed_single: &mut consumed_single,
        consumed_supervisor: &mut consumed_supervisor,
        cancel: cancel.clone(),
    };
    let result = super::session_inner::run_chat_inner(&mut run_ctx, &run_req).await;

    // Strip images_base64 from all messages after each chat round.
    // These base64 payloads (screenshots from computer agent) are wire-only and
    // can be 500 KB–2 MB each. Keeping them in the history Vec across turns
    // causes unbounded memory growth in long-running sessions.
    crate::chat_service::util::strip_images_from_history(&mut history);

    if let Err(e) = crate::token_usage_store::finalize_run(&run_id, &conversation_id, &history) {
        log::warn!(
            "token_usage_store: finalize_run failed run_id={run_id} conversation_id={conversation_id}: {e}"
        );
    }
    if let Err(e) =
        crate::token_usage_store::flush_unsent_reports(&state.active_platform_auth()).await
    {
        log::warn!("token_usage_store: flush after chat failed: {e}");
    }

    if let Some(session) = transcript_session.as_ref() {
        crate::conversation_transcript::ConversationTranscriptSession::end(&history, session);
    } else {
        super::conversation_persist::append_missing(&conversation_id, &history);
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
                conversation_id: conversation_id.clone(),
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
    // Persist per-turn consumption only; the cap is evaluated from the latest
    // user prompt (each run_chat starts a fresh budget at 0).
    super::conversation_persist::patch_tool_rounds(
        &conversation_id,
        consumed_single,
        consumed_supervisor,
        super::util::now_ms(),
    );
    log::info!(
        "run_chat conversation end: emitting StreamEvent::Done conversation_id={} run_outcome={} history_messages_final={} consumed_this_run_single={} consumed_this_run_supervisor={} max_tool_rounds_attached={}",
        conversation_id,
        if result.is_ok() { "Ok" } else { "Err" },
        history.len(),
        consumed_single,
        consumed_supervisor,
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
    emit(
        &stream,
        StreamEvent::Done {
            conversation_id,
            tool_rounds_used_total: Some(consumed_single),
            tool_rounds_used_supervisor_total: Some(consumed_supervisor),
            max_tool_rounds: Some(max_tr),
        },
    );
    log::info!(
        "run_chat finished after Done emit final_result_is_ok={}",
        result.is_ok(),
    );
    result
}
