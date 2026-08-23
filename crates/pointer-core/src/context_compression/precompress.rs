//! Background precompress, pending splice, and between-round prepare.

use super::budget::*;
use super::run::{
    clear_last_lead_prompt_tokens, compress_history_inner, fingerprint_window_start,
    maybe_compress_history, splice_summary_into_drop_window,
};
use super::types::*;
use crate::agent_instance_scope::AgentInstanceScope;
use crate::models::{ChatMessage, ModelSettings, StreamEvent};
use crate::provider::OpenAIProvider;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use tokio_util::sync::CancellationToken;

pub(crate) struct PendingCompressionSplice {
    pub(crate) queue_key: String,
    pub(crate) conversation_id: String,
    pub(crate) ui: CompressionUiContext,
    pub(crate) fingerprint_prefix_ids: Vec<String>,
    pub(crate) insert_before_message_id: String,
    pub(crate) excluded_for_persist: Vec<ChatMessage>,
    pub(crate) summary_msg: ChatMessage,
    pub(crate) preview_for_disk: String,
    pub(crate) dropped_count: u32,
    pub(crate) keep_users: u32,
    pub(crate) apply_reason: String,
    pub(crate) summary_failed: bool,
}

pub(crate) struct PrecompressCoordinator {
    /// `false` while running, `true` when finished (success or skip).
    inflight: Mutex<HashMap<String, tokio::sync::watch::Receiver<bool>>>,
    pending: Mutex<HashMap<String, PendingCompressionSplice>>,
    /// Last resolved lead LLM for a conversation (same provider/model as `run_chat`).
    session_llm: Mutex<HashMap<String, SessionLlmSnapshot>>,
}

#[derive(Clone)]
pub(crate) struct SessionLlmSnapshot {
    pub settings: ModelSettings,
    pub api_key: String,
}

impl PrecompressCoordinator {
    fn global() -> &'static Self {
        static COORD: OnceLock<PrecompressCoordinator> = OnceLock::new();
        COORD.get_or_init(|| Self {
            inflight: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            session_llm: Mutex::new(HashMap::new()),
        })
    }
}

/// Remember the lead LLM this conversation just used, so background compression
/// does not re-resolve from global settings (which can pick a different provider).
pub fn remember_session_llm(conversation_id: &str, settings: &ModelSettings, api_key: &str) {
    let id = conversation_id.trim();
    if id.is_empty() {
        log::warn!("context_compress: remember session llm skipped (empty conversation_id)");
        return;
    }
    log::info!(
        "context_compress: remember session llm conversation_id={} provider={} model={}",
        id,
        settings.active_provider_id,
        settings.model
    );
    PrecompressCoordinator::global().session_llm.lock().insert(
        id.to_string(),
        SessionLlmSnapshot {
            settings: settings.clone(),
            api_key: api_key.to_string(),
        },
    );
}

pub(crate) fn session_llm_for_conversation(conversation_id: &str) -> Option<SessionLlmSnapshot> {
    PrecompressCoordinator::global()
        .session_llm
        .lock()
        .get(conversation_id.trim())
        .cloned()
}

pub(crate) fn snapshot_from_provider(provider: &OpenAIProvider) -> SessionLlmSnapshot {
    SessionLlmSnapshot {
        settings: provider.settings.clone(),
        api_key: provider.api_key.clone(),
    }
}

pub(crate) fn enqueue_pending_splice(pending: PendingCompressionSplice) {
    let id = pending.queue_key.clone();
    PrecompressCoordinator::global()
        .pending
        .lock()
        .insert(id, pending);
}

pub(crate) fn discard_pending_by_key(key: &str) {
    let id = key.trim();
    if id.is_empty() {
        return;
    }
    if PrecompressCoordinator::global()
        .pending
        .lock()
        .remove(id)
        .is_some()
    {
        log::info!("context_compress: discarded pending splice key={id}");
    }
}

/// Drop a queued splice (e.g. before overflow sync compress).
pub fn discard_pending_compression(conversation_id: &str) {
    discard_pending_by_key(conversation_id);
}

pub(crate) fn discard_pending_compression_for_sub_agent(
    conversation_id: &str,
    agent_instance_id: &str,
) {
    discard_pending_by_key(&sub_agent_compression_queue_key(
        conversation_id,
        agent_instance_id,
    ));
}

pub(crate) fn pending_fingerprint_matches(
    history: &[ChatMessage],
    pending: &PendingCompressionSplice,
) -> bool {
    fingerprint_window_start(history, &pending.fingerprint_prefix_ids).is_some()
}

/// Apply a queued precompress splice when the conversation is idle.
/// Call after `cancels` is cleared for this conversation.
pub fn try_apply_pending_compression(
    state: &crate::chat_service::AppState,
    conversation_id: &str,
) -> bool {
    let id = conversation_id.trim();
    if id.is_empty() {
        return false;
    }
    if state.cancels.lock().contains_key(id) {
        log::debug!("context_compress: pending apply skipped (still busy) conversation_id={id}");
        return false;
    }
    let Some(pending) = PrecompressCoordinator::global().pending.lock().remove(id) else {
        return false;
    };
    let Ok(store) = crate::conversation_store::global_store() else {
        log::warn!("context_compress: pending apply no store conversation_id={id}");
        return false;
    };
    let (history, _db_messages) = match store.load_lead_working_messages(id) {
        Ok((working, db_count)) => (working, db_count),
        Err(e) => {
            log::warn!("context_compress: pending apply load failed conversation_id={id}: {e:#}");
            return false;
        }
    };
    if !pending_fingerprint_matches(&history, &pending) {
        log::warn!(
            "context_compress: pending splice stale, discarded conversation_id={id} prefix_ids={}",
            pending.fingerprint_prefix_ids.len()
        );
        return false;
    }
    crate::conversation_transcript::persist_compression_splice(
        id,
        &pending.excluded_for_persist,
        &pending.summary_msg,
        &pending.insert_before_message_id,
        &pending.preview_for_disk,
    );
    clear_last_lead_prompt_tokens(id);
    if let Err(e) = state.memory_store.reload_snapshot_for_conversation(id) {
        log::warn!("memory: reload after pending compression failed: {e:#}");
    }
    let stream_tx = crate::models::ChatStreamSender::unbound(
        id,
        crate::user_storage::session_user_id_for_conversation(id),
    );
    let ui = CompressionUiContext {
        scope: CompressionScope::Main,
        ..Default::default()
    };
    let (done_msg, done_level) = compression_done_toast(
        &ui,
        pending.dropped_count,
        pending.keep_users,
        pending.summary_failed,
    );
    emit_ui_toast(&stream_tx, id, &done_msg, done_level);
    let compression = build_compression_info(
        &ui,
        &pending.apply_reason,
        pending.fingerprint_prefix_ids.len(),
        0,
        pending.dropped_count,
        pending.keep_users,
    );
    crate::stream_broadcast::publish_stream(
        &stream_tx,
        StreamEvent::ContextCompressionApplied {
            conversation_id: id.to_string(),
            compression,
            excluded_message_ids: pending
                .excluded_for_persist
                .iter()
                .map(|m| m.id.clone())
                .collect(),
            summary_message: pending.summary_msg.clone(),
            insert_before_message_id: pending.insert_before_message_id,
        },
    );
    log::info!(
        "context_compress: pending splice applied conversation_id={id} dropped={}",
        pending.dropped_count
    );
    true
}

pub(crate) fn splice_pending_into_history(
    history: &mut Vec<ChatMessage>,
    pending: &PendingCompressionSplice,
) -> bool {
    let Some(drop_start) = fingerprint_window_start(history, &pending.fingerprint_prefix_ids)
    else {
        return false;
    };
    let drop_end = drop_start + pending.fingerprint_prefix_ids.len();
    let drop_ids: HashSet<String> = pending
        .excluded_for_persist
        .iter()
        .map(|m| m.id.clone())
        .collect();
    splice_summary_into_drop_window(
        history,
        drop_start,
        drop_end,
        &drop_ids,
        pending.summary_msg.clone(),
    );
    true
}

/// After a live pending splice, last-round `usage.prompt_tokens` describes the
/// pre-splice request. Later gates must re-estimate from current history.
pub(crate) fn prompt_tokens_after_pending_apply(
    applied: bool,
    reported_prompt_tokens: Option<u32>,
) -> Option<u32> {
    if applied {
        None
    } else {
        reported_prompt_tokens
    }
}

/// Apply a queued precompress splice onto the live working set, even while
/// the current turn is still running. Safe when the compressed prefix is
/// unchanged and new messages were only appended after the split.
pub fn try_apply_pending_compression_live(
    state: &crate::chat_service::AppState,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    stream: &StreamTx,
) -> bool {
    try_apply_pending_keyed(conversation_id.trim(), history, stream, Some(state))
}

pub(crate) fn try_apply_pending_keyed(
    key: &str,
    history: &mut Vec<ChatMessage>,
    stream: &StreamTx,
    lead_state: Option<&crate::chat_service::AppState>,
) -> bool {
    if key.is_empty() {
        return false;
    }
    let Some(pending) = PrecompressCoordinator::global().pending.lock().remove(key) else {
        return false;
    };
    let conversation_id = pending.conversation_id.clone();
    if !splice_pending_into_history(history, &pending) {
        log::warn!(
            "context_compress: live pending splice stale, discarded key={key} prefix_ids={}",
            pending.fingerprint_prefix_ids.len()
        );
        return false;
    }
    if matches!(pending.ui.scope, CompressionScope::Main) {
        let preview_for_disk = crate::conversation_store::conversation_preview(history);
        crate::conversation_transcript::persist_compression_splice(
            &conversation_id,
            &pending.excluded_for_persist,
            &pending.summary_msg,
            &pending.insert_before_message_id,
            &preview_for_disk,
        );
        crate::conversation_session::publish_working_set(&conversation_id, history, None);
        clear_last_lead_prompt_tokens(&conversation_id);
        if let Some(state) = lead_state {
            if let Err(e) = state
                .memory_store
                .reload_snapshot_for_conversation(&conversation_id)
            {
                log::warn!("memory: reload after live pending compression failed: {e:#}");
            }
        }
    }
    emit_pending_applied(stream, &conversation_id, history.len(), &pending);
    log::info!(
        "context_compress: live pending splice applied key={key} dropped={}",
        pending.dropped_count
    );
    true
}

pub(crate) fn emit_pending_applied(
    stream: &StreamTx,
    conversation_id: &str,
    history_len: usize,
    pending: &PendingCompressionSplice,
) {
    let ui = &pending.ui;
    let (done_msg, done_level) = compression_done_toast(
        ui,
        pending.dropped_count,
        pending.keep_users,
        pending.summary_failed,
    );
    emit_ui_toast(stream, conversation_id, &done_msg, done_level);
    let compression = build_compression_info(
        ui,
        &pending.apply_reason,
        pending.fingerprint_prefix_ids.len(),
        history_len,
        pending.dropped_count,
        pending.keep_users,
    );
    match ui.scope {
        CompressionScope::Main => {
            crate::stream_broadcast::publish_stream(
                stream,
                StreamEvent::ContextCompressionApplied {
                    conversation_id: conversation_id.to_string(),
                    compression,
                    excluded_message_ids: pending
                        .excluded_for_persist
                        .iter()
                        .map(|m| m.id.clone())
                        .collect(),
                    summary_message: pending.summary_msg.clone(),
                    insert_before_message_id: pending.insert_before_message_id.clone(),
                },
            );
        }
        CompressionScope::SubAgent => {
            if let Some(message_id) = ui.message_id.as_deref() {
                crate::stream_broadcast::publish_stream(
                    stream,
                    StreamEvent::ContextCompressed {
                        conversation_id: conversation_id.to_string(),
                        message_id: message_id.to_string(),
                        compression,
                    },
                );
            }
        }
    }
}

pub(crate) async fn await_inflight_precompress(queue_key: &str) {
    let rx = PrecompressCoordinator::global()
        .inflight
        .lock()
        .get(queue_key)
        .cloned();
    let Some(mut rx) = rx else {
        return;
    };
    if *rx.borrow() {
        return;
    }
    match tokio::time::timeout(std::time::Duration::from_secs(120), rx.changed()).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => {
            log::warn!("context_compress: inflight watch closed key={queue_key}");
        }
        Err(_) => {
            log::warn!("context_compress: inflight wait timed out key={queue_key}");
        }
    }
}

pub(crate) fn spawn_sub_agent_precompress(
    history: &[ChatMessage],
    provider: &OpenAIProvider,
    conversation_id: &str,
    stream: StreamTx,
    cancel: CancellationToken,
    ui: CompressionUiContext,
    reported_prompt_tokens: Option<u32>,
    lease: &SubAgentPrecompressLease,
) {
    let key = lease.key().to_string();
    let live = lease.live();
    let coord = PrecompressCoordinator::global();
    let (done_tx, done_rx) = tokio::sync::watch::channel(false);
    {
        let mut map = coord.inflight.lock();
        if map.contains_key(&key) {
            log::info!("context_compress: sub_agent precompress already in flight key={key}");
            return;
        }
        map.insert(key.clone(), done_rx);
    }
    let mut snapshot = history.to_vec();
    let settings = provider.settings.clone();
    let api_key = provider.api_key.clone();
    let cid = conversation_id.to_string();
    log::info!(
        "context_compress: sub_agent precompress spawn key={key} share={:.3} messages={}",
        current_turn_token_share(history),
        history.len()
    );
    tokio::spawn(async move {
        let applied = async {
            if !live.load(Ordering::SeqCst) {
                return false;
            }
            if api_key.trim().is_empty() {
                log::info!(
                    "context_compress: sub_agent precompress skipped (no api key) key={key}"
                );
                return false;
            }
            let provider = OpenAIProvider::new(settings.clone(), api_key);
            compress_history_inner(
                &mut snapshot,
                &settings,
                &provider,
                &cid,
                &stream,
                cancel,
                false,
                false,
                true,
                true,
                &ui,
                reported_prompt_tokens,
                None,
                true,
            )
            .await;
            if !live.load(Ordering::SeqCst) {
                discard_pending_by_key(&key);
                return false;
            }
            true
        }
        .await;
        log::info!("context_compress: sub_agent precompress job finished key={key} ok={applied}");
        let _ = done_tx.send(true);
        PrecompressCoordinator::global()
            .inflight
            .lock()
            .remove(&key);
    });
}

/// Sub-agent tool loop: same 80% / 100% gates as lead. Soft gate spawns an
/// isolated background job (queue key is not the lead conversation id).
pub(crate) async fn prepare_sub_agent_history_between_llm_rounds(
    history: &mut Vec<ChatMessage>,
    settings: &ModelSettings,
    provider: &OpenAIProvider,
    conversation_id: &str,
    stream: &StreamTx,
    cancel: CancellationToken,
    ui: CompressionUiContext,
    reported_prompt_tokens: Option<u32>,
    lease: &SubAgentPrecompressLease,
) {
    if !settings.context_compression_enabled {
        return;
    }
    if ui.scope != CompressionScope::SubAgent {
        log::warn!(
            "context_compress: prepare_sub_agent_history called with non-sub scope conversation_id={}",
            conversation_id
        );
        return;
    }
    let mut tokens = reported_prompt_tokens;
    if try_apply_pending_keyed(lease.key(), history, stream, None) {
        tokens = prompt_tokens_after_pending_apply(true, tokens);
        log::info!(
            "context_compress: stale prompt_tokens discarded after pending splice conversation_id={} scope=sub_agent",
            conversation_id
        );
    }
    let budget = normalize_context_budget_tokens(crate::models::effective_context_budget_tokens(settings));
    let keep_users = settings.context_keep_recent_user_turns.max(1) as usize;
    match sub_agent_between_round_compress_soft(history, tokens, budget, keep_users) {
        None => {}
        Some(false) => {
            log::info!(
                "context_compress: sub_agent between-round hard gate conversation_id={} share={:.3} messages={}",
                conversation_id,
                current_turn_token_share(history),
                history.len()
            );
            await_inflight_precompress(lease.key()).await;
            if try_apply_pending_keyed(lease.key(), history, stream, None) {
                tokens = prompt_tokens_after_pending_apply(true, tokens);
                log::info!(
                    "context_compress: stale prompt_tokens discarded after pending splice conversation_id={} scope=sub_agent",
                    conversation_id
                );
            }
            let still = sub_agent_between_round_compress_soft(history, tokens, budget, keep_users);
            if matches!(still, Some(false)) && !cancel.is_cancelled() {
                let _ = maybe_compress_history(
                    history,
                    settings,
                    provider,
                    conversation_id,
                    stream,
                    cancel,
                    ui,
                    None,
                    tokens,
                )
                .await;
            }
        }
        Some(true) => {
            spawn_sub_agent_precompress(
                history,
                provider,
                conversation_id,
                stream.clone(),
                cancel,
                ui,
                tokens,
                lease,
            );
        }
    }
}

/// Between LLM rounds: apply any ready async splice; spawn background
/// compression when approaching the budget; only block when already over.
pub async fn prepare_history_between_llm_rounds(
    state: Arc<crate::chat_service::AppState>,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    settings: &ModelSettings,
    provider: &OpenAIProvider,
    stream: &StreamTx,
    cancel: CancellationToken,
    ui: CompressionUiContext,
    reported_prompt_tokens: Option<u32>,
) {
    if !settings.context_compression_enabled {
        return;
    }
    let mut tokens = reported_prompt_tokens;
    if try_apply_pending_compression_live(state.as_ref(), conversation_id, history, stream) {
        tokens = prompt_tokens_after_pending_apply(true, tokens);
        log::info!(
            "context_compress: stale prompt_tokens discarded after pending splice conversation_id={} scope=main",
            conversation_id
        );
    }
    let budget = normalize_context_budget_tokens(crate::models::effective_context_budget_tokens(settings));
    let keep_users = settings.context_keep_recent_user_turns.max(1) as usize;
    let hard_plan = plan_compression(history, tokens, budget, keep_users, false, false, false);
    if !matches!(hard_plan, CompressionPlan::Skip) {
        log::info!(
            "context_compress: between-round hard gate conversation_id={} plan={:?} share={:.3}",
            conversation_id,
            hard_plan,
            current_turn_token_share(history)
        );
        await_inflight_precompress(conversation_id).await;
        if try_apply_pending_compression_live(state.as_ref(), conversation_id, history, stream) {
            tokens = prompt_tokens_after_pending_apply(true, tokens);
            log::info!(
                "context_compress: stale prompt_tokens discarded after pending splice conversation_id={} scope=main",
                conversation_id
            );
        }
        let still = plan_compression(history, tokens, budget, keep_users, false, false, false);
        if !matches!(still, CompressionPlan::Skip) && !cancel.is_cancelled() {
            let _ = maybe_compress_history(
                history,
                settings,
                provider,
                conversation_id,
                stream,
                cancel,
                ui,
                None,
                tokens,
            )
            .await;
        }
        return;
    }
    maybe_spawn_precompress_from_history(
        state,
        conversation_id.to_string(),
        history,
        tokens,
        provider,
    );
}

/// After a turn ends, optionally start soft-threshold compression in the background.
pub fn maybe_spawn_precompress(state: Arc<crate::chat_service::AppState>, conversation_id: String) {
    let id = conversation_id.trim().to_string();
    if id.is_empty() {
        return;
    }
    let Ok(store) = crate::conversation_store::global_store() else {
        log::warn!("context_compress: precompress spawn skipped (no store) conversation_id={id}");
        return;
    };
    let (mut history, db_messages) = match store.load_lead_working_messages(&id) {
        Ok((working, db_count)) => (working, db_count),
        Err(e) => {
            log::warn!(
                "context_compress: precompress spawn load_working failed conversation_id={id}: {e:#}"
            );
            return;
        }
    };
    crate::chat_service::sub_message::strip_scoped_from_lead_history(&mut history);
    let last_api = store.get_last_lead_prompt_tokens(&id).ok().flatten();
    let Some(llm) = session_llm_for_conversation(&id) else {
        log::warn!(
            "context_compress: precompress spawn skipped (no session llm) conversation_id={id}"
        );
        return;
    };
    spawn_precompress_if_soft_gate(state, id, &history, last_api, Some(db_messages), llm);
}

/// Same as [`maybe_spawn_precompress`], gated on the live in-memory history
/// (used between LLM rounds while a turn is still open).
pub fn maybe_spawn_precompress_from_history(
    state: Arc<crate::chat_service::AppState>,
    conversation_id: String,
    history: &[ChatMessage],
    reported_prompt_tokens: Option<u32>,
    provider: &OpenAIProvider,
) {
    let id = conversation_id.trim().to_string();
    if id.is_empty() {
        return;
    }
    let llm = snapshot_from_provider(provider);
    remember_session_llm(&id, &llm.settings, &llm.api_key);
    spawn_precompress_if_soft_gate(state, id, history, reported_prompt_tokens, None, llm);
}

pub(crate) fn spawn_precompress_if_soft_gate(
    state: Arc<crate::chat_service::AppState>,
    id: String,
    history: &[ChatMessage],
    reported_prompt_tokens: Option<u32>,
    db_messages: Option<u32>,
    llm: SessionLlmSnapshot,
) {
    if !llm.settings.context_compression_enabled {
        return;
    }
    let budget = normalize_context_budget_tokens(crate::models::effective_context_budget_tokens(
        &llm.settings,
    ));
    let keep_users = llm.settings.context_keep_recent_user_turns.max(1) as usize;
    let plan = plan_compression(
        history,
        reported_prompt_tokens,
        budget,
        keep_users,
        true,
        false,
        false,
    );
    if matches!(plan, CompressionPlan::Skip) {
        log::debug!(
            "context_compress: precompress spawn not needed conversation_id={id} plan=Skip share={:.3}",
            current_turn_token_share(history)
        );
        return;
    }

    let coord = PrecompressCoordinator::global();
    let (done_tx, done_rx) = tokio::sync::watch::channel(false);
    {
        let mut map = coord.inflight.lock();
        if map.contains_key(&id) {
            log::info!("context_compress: precompress already in flight conversation_id={id}");
            return;
        }
        map.insert(id.clone(), done_rx);
    }

    log::info!(
        "context_compress: precompress spawn conversation_id={id} plan={:?} share={:.3} budget={} messages={} db_messages={} provider={} model={}",
        plan,
        current_turn_token_share(history),
        budget,
        history.len(),
        db_messages.map(|n| n.to_string()).unwrap_or_else(|| "-".into()),
        llm.settings.active_provider_id,
        llm.settings.model
    );
    tokio::spawn(async move {
        let outcome = run_precompress_job(state, &id, llm).await;
        log::info!(
            "context_compress: precompress job finished conversation_id={id} applied={outcome}"
        );
        let _ = done_tx.send(true);
        PrecompressCoordinator::global().inflight.lock().remove(&id);
    });
}

pub(crate) async fn run_precompress_job(
    state: Arc<crate::chat_service::AppState>,
    conversation_id: &str,
    llm: SessionLlmSnapshot,
) -> bool {
    let Ok(store) = crate::conversation_store::global_store() else {
        log::warn!(
            "context_compress: precompress skipped (no store) conversation_id={conversation_id}"
        );
        return false;
    };
    let (mut history, _db_messages) = match store.load_lead_working_messages(conversation_id) {
        Ok((working, db_count)) => (working, db_count),
        Err(e) => {
            log::warn!(
                "context_compress: precompress load_working failed conversation_id={conversation_id}: {e:#}"
            );
            return false;
        }
    };
    crate::chat_service::sub_message::strip_scoped_from_lead_history(&mut history);

    let settings = llm.settings;
    let api_key = llm.api_key;
    if api_key.trim().is_empty() {
        log::info!(
            "context_compress: precompress skipped (no api key) conversation_id={conversation_id}"
        );
        return false;
    }
    let budget = normalize_context_budget_tokens(crate::models::effective_context_budget_tokens(
        &settings,
    ));
    let keep_users = settings.context_keep_recent_user_turns.max(1) as usize;
    let last_api = store
        .get_last_lead_prompt_tokens(conversation_id)
        .ok()
        .flatten();
    let plan = plan_compression(&history, last_api, budget, keep_users, true, false, false);
    if matches!(plan, CompressionPlan::Skip) {
        log::debug!(
            "context_compress: precompress not needed conversation_id={conversation_id} plan=Skip share={:.3}",
            current_turn_token_share(&history)
        );
        return false;
    }

    let provider = OpenAIProvider::new(settings.clone(), api_key);
    let stream_tx = crate::models::ChatStreamSender::unbound(
        conversation_id,
        crate::user_storage::session_user_id_for_conversation(conversation_id),
    );
    let cancel = CancellationToken::new();
    let run_id = format!("precompress-{}", uuid::Uuid::new_v4().simple());
    let lead_role = if settings.lead_agent_id.trim().is_empty() {
        settings.agent_mode.clone()
    } else {
        settings.lead_agent_id.clone()
    };
    let ui = CompressionUiContext::main(AgentInstanceScope::new(
        run_id,
        conversation_id.to_string(),
        lead_role,
    ));

    log::info!(
        "context_compress: precompress starting conversation_id={conversation_id} plan={:?} share={:.3} messages={} provider={} model={}",
        plan,
        current_turn_token_share(&history),
        history.len(),
        settings.active_provider_id,
        settings.model
    );
    let changed = compress_history_inner(
        &mut history,
        &settings,
        &provider,
        conversation_id,
        &stream_tx,
        cancel,
        false,
        false,
        true, // soft_precompress
        true, // emit UI via global broadcast
        &ui,
        last_api,
        Some(state.clone()),
        false,
    )
    .await;
    if changed {
        clear_last_lead_prompt_tokens(conversation_id);
        if let Err(e) = state
            .memory_store
            .reload_snapshot_for_conversation(conversation_id)
        {
            log::warn!("memory: reload after precompress failed: {e:#}");
        }
    }
    changed
}
