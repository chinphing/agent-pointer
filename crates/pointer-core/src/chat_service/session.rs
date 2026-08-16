//! Session entry: cancel registration, `run_chat_inner`, `Done` / error streaming.

use crate::dispatcher::TriggerSource;
use crate::models::{ChatMessage, Role, StreamEvent};
use crate::provider::OpenAIProvider;
use anyhow::Result;
use std::backtrace::Backtrace;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use uuid::Uuid;

use super::app_state::AppState;
use super::emit::emit;
use super::session_model::prepare_session_llm_settings;
use super::StreamTx;

fn assistant_stream_started(before: &[ChatMessage], after: &[ChatMessage]) -> bool {
    let before_ids: std::collections::HashSet<&str> =
        before.iter().map(|m| m.id.as_str()).collect();
    for m in after {
        if !matches!(m.role, Role::Assistant) {
            continue;
        }
        let is_new = !before_ids.contains(m.id.as_str());
        let has_body = !m.content.trim().is_empty()
            || m.reasoning.as_ref().is_some_and(|r| !r.trim().is_empty())
            || m.tool_calls.as_ref().is_some_and(|t| !t.is_empty());
        let streaming = m.status == "streaming" || m.status == "pending";
        if is_new && (has_body || streaming) {
            return true;
        }
        if !is_new && has_body {
            // Existing assistant row gained content during this turn.
            if let Some(old) = before.iter().find(|x| x.id == m.id) {
                let tools_changed = old.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0)
                    != m.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0);
                if old.content != m.content || old.reasoning != m.reasoning || tools_changed {
                    return true;
                }
            }
        }
    }
    false
}

pub async fn run_chat(
    stream: StreamTx,
    state: Arc<AppState>,
    conversation_id: String,
    mut history: Vec<ChatMessage>,
    _enabled_skill_ids: Vec<String>,
    agent_skill_overrides: HashMap<String, Vec<String>>,
    agent_mode: Option<String>,
    lead_agent_id_override: Option<String>,
    performance_mode_override: Option<String>,
    tool_rounds_used_single_start: u32,
    tool_rounds_used_supervisor_start: u32,
    workspace_root: String,
    workspace_inherit_disabled: Option<bool>,
    trigger_source: Option<TriggerSource>,
    im_auto_deliver: bool,
) -> Result<()> {
    // 本轮 AI 工作区间起点（排除前端 dispatch / 排队 / 网络传输），Done 事件携带。
    let run_started_at_ms = super::util::now_ms();
    // Canonical skill source: user_settings.agentSkillOverrides (request overrides
    // only when non-empty, e.g. tests). Legacy enabledSkillIds is ignored.
    let agent_skill_overrides = if agent_skill_overrides.is_empty() {
        state.default_run_agent_skill_overrides()
    } else {
        agent_skill_overrides
    };
    // Filled with the lead's resolved skill ids after build_plan (for inherit/import).
    let mut enabled_skill_ids: Vec<String> = Vec::new();
    log::info!(
        "run_chat start conversation_id={} incoming_history_messages={} agent_skill_overrides={} request_agent_mode={:?} lead_agent_id_override={:?} tool_rounds_used_single_start={} tool_rounds_used_supervisor_start={} trigger_source={:?} im_auto_deliver={}",
        conversation_id,
        history.len(),
        agent_skill_overrides.len(),
        agent_mode,
        lead_agent_id_override,
        tool_rounds_used_single_start,
        tool_rounds_used_supervisor_start,
        trigger_source,
        im_auto_deliver,
    );

    super::sub_message::strip_scoped_from_lead_history(&mut history);

    state.touch_activity();

    let cancel = CancellationToken::new();
    state
        .cancels
        .lock()
        .insert(conversation_id.clone(), cancel.clone());

    // Do not await precompress — send path stays non-blocking. Busy precompress
    // enqueues splice for idle apply after this turn.

    let transcript_session =
        match crate::conversation_transcript::ConversationTranscriptSession::begin(
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
        agent_mode: agent_mode.clone(),
        lead_agent_id_override: lead_agent_id_override.clone(),
        performance_mode_override: performance_mode_override.clone(),
        agent_skill_overrides: agent_skill_overrides.clone(),
        tool_rounds_used_single_start,
        workspace_root: workspace_root.clone(),
        workspace_inherit_disabled,
        run_id: run_id.clone(),
        trigger_source,
        im_auto_deliver,
    };
    let history_before = history.clone();
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
    let mut result = super::session_inner::run_chat_inner(&mut run_ctx, &run_req).await;

    // Overflow → sync compress once → retry same turn if no assistant stream yet.
    if let Err(err) = &result {
        if crate::context_compression::is_context_overflow_error(err) {
            let settings = state.effective_settings();
            if settings.context_compression_enabled {
                let started = assistant_stream_started(&history_before, &history);
                log::warn!(
                    "run_chat: context overflow conversation_id={} assistant_stream_started={} err={err:#}",
                    conversation_id,
                    started
                );
                crate::context_compression::discard_pending_compression(&conversation_id);
                let mut llm_settings = settings.clone();
                let mode = agent_mode
                    .clone()
                    .unwrap_or_else(|| llm_settings.agent_mode.clone());
                let api_key = prepare_session_llm_settings(
                    &mut llm_settings,
                    &mode,
                    lead_agent_id_override.as_deref(),
                    performance_mode_override.as_deref(),
                );
                if !api_key.trim().is_empty() {
                    let provider = OpenAIProvider::new(llm_settings.clone(), api_key);
                    let last_api =
                        crate::conversation_store::global_store()
                            .ok()
                            .and_then(|store| {
                                store
                                    .get_last_lead_prompt_tokens(&conversation_id)
                                    .ok()
                                    .flatten()
                            });
                    let lead_role = lead_agent_id_override
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| mode.clone());
                    let ui = crate::context_compression::CompressionUiContext::main(
                        crate::agent_instance_scope::AgentInstanceScope::new(
                            format!("overflow-{}", Uuid::new_v4().simple()),
                            conversation_id.clone(),
                            lead_role,
                        ),
                    );
                    emit(
                        &stream,
                        StreamEvent::UiToast {
                            conversation_id: conversation_id.clone(),
                            message: if started {
                                "上下文超限，正在压缩（请之后重发）".into()
                            } else {
                                "上下文超限，正在压缩后重试".into()
                            },
                            level: "warning".into(),
                        },
                    );
                    let compressed = crate::context_compression::maybe_compress_history(
                        &mut history,
                        &llm_settings,
                        &provider,
                        &conversation_id,
                        &stream,
                        cancel.clone(),
                        ui,
                        Some(state.memory_store.as_ref()),
                        last_api,
                    )
                    .await;
                    if compressed && !started && !cancel.is_cancelled() {
                        log::info!(
                            "run_chat: retrying after overflow compress conversation_id={}",
                            conversation_id
                        );
                        enabled_skill_ids.clear();
                        consumed_single = 0;
                        consumed_supervisor = 0;
                        let mut retry_ctx = super::context::ChatRunContext {
                            stream: stream.clone(),
                            state: state.clone(),
                            conversation_id: &conversation_id,
                            history: &mut history,
                            enabled_skill_ids: &mut enabled_skill_ids,
                            consumed_single: &mut consumed_single,
                            consumed_supervisor: &mut consumed_supervisor,
                            cancel: cancel.clone(),
                        };
                        result =
                            super::session_inner::run_chat_inner(&mut retry_ctx, &run_req).await;
                    } else if !compressed && !started {
                        log::warn!(
                            "run_chat: overflow but compress did not apply (keep window too large?) conversation_id={}",
                            conversation_id
                        );
                        emit(
                            &stream,
                            StreamEvent::UiToast {
                                conversation_id: conversation_id.clone(),
                                message: "上下文过大且无法压缩保留区，请新开对话或删减内容".into(),
                                level: "error".into(),
                            },
                        );
                    }
                }
            }
        }
    }

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

    // Idle: apply any precompress splice queued while this turn was active.
    let _ = crate::context_compression::try_apply_pending_compression(&state, &conversation_id);

    if let Err(err) = &result {
        // Root cause is in `err` (often an HTTP/API message). `Backtrace::capture()` here only
        // shows the async poll point (e.g. chat_service + tokio), not the failing await site.
        log::error!(
            "run_chat failed conversation_id={} error={:#}",
            conversation_id,
            err
        );
        log::debug!(
            "run_chat failure poll-point backtrace (for deep debugging):\n{}",
            Backtrace::capture()
        );
        let (message, message_id) = super::emit::chat_run_error_parts(err);
        emit(
            &stream,
            StreamEvent::Error {
                conversation_id: conversation_id.clone(),
                message_id,
                message,
            },
        );
        log::info!(
            "run_chat emitted StreamEvent::Error conversation_id={} message_len_chars={}",
            conversation_id,
            err.to_string().chars().count(),
        );
    }
    let max_tr = state.effective_settings().max_tool_rounds;
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
        && crate::task_board::maybe_auto_finalize_if_complete(
            &state.task_board_store,
            &main_store_key,
        )
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
            conversation_id: conversation_id.clone(),
            tool_rounds_used_total: Some(consumed_single),
            tool_rounds_used_supervisor_total: Some(consumed_supervisor),
            max_tool_rounds: Some(max_tr),
            started_at_ms: Some(run_started_at_ms),
            finished_at_ms: Some(super::util::now_ms()),
        },
    );
    log::info!(
        "run_chat finished after Done emit final_result_is_ok={}",
        result.is_ok(),
    );
    // Soft-threshold background compress while the user is idle / composing.
    if result.is_ok() {
        crate::context_compression::maybe_spawn_precompress(state.clone(), conversation_id);
    }
    result
}
