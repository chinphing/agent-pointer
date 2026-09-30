//! Single entry for in-flight conversation transcript mutations during `run_chat`.
//!
//! Canonical on disk: `role: tool` rows immediately after the assistant that issued
//! `tool_calls`. In-memory lead `history` is the LLM working set (`context_state.included`);
//! soft-excluded rows stay in SQLite for UI hydrate. DB is synced in batches.

mod reconcile;
mod registry;

use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use anyhow::Result;
use parking_lot::Mutex;

use crate::conversation_store::{self, conversation_preview};
use crate::models::ChatMessage;

pub use registry::ConversationTranscriptRegistry;

static REGISTRY: OnceLock<Arc<ConversationTranscriptRegistry>> = OnceLock::new();

pub fn global_registry() -> Arc<ConversationTranscriptRegistry> {
    REGISTRY
        .get_or_init(|| Arc::new(ConversationTranscriptRegistry::new()))
        .clone()
}

/// Drop scoped sub-agent rows and soft-excluded messages from a lead history snapshot.
#[cfg(test)]
pub(crate) fn filter_lead_working_history(
    messages: impl IntoIterator<Item = ChatMessage>,
) -> Vec<ChatMessage> {
    messages
        .into_iter()
        .filter(|message| !crate::models::is_scoped_sub_message(message))
        .filter(|message| crate::message_context::is_context_included(message))
        .collect()
}

/// Meta `message_count` must reflect the full SQLite transcript (including soft-excluded
/// orphans), never the short in-memory working set length alone.
fn meta_message_count(conversation_id: &str, working_len: u32) -> u32 {
    conversation_store::global_store()
        .ok()
        .and_then(|store| store.message_count(conversation_id).ok())
        .map(|db| db.max(working_len))
        .unwrap_or(working_len)
}

/// Active transcript session for one `run_chat` invocation.
pub struct ConversationTranscriptSession {
    conversation_id: String,
    message_count: u32,
    preview: String,
    known_ids: HashSet<String>,
    transcript_dirty: bool,
}

impl ConversationTranscriptSession {
    pub fn begin(
        conversation_id: &str,
        history: &mut Vec<ChatMessage>,
    ) -> Result<Arc<Mutex<Self>>> {
        // Unified session facade: append deltas + working-set cache / DB reload.
        let db_message_count =
            crate::conversation_session::prepare_lead_history(conversation_id, history)?;

        let reconciled = reconcile::reconcile_tool_messages(history);
        let message_count = db_message_count.max(history.len() as u32);
        let preview = conversation_preview(history);
        let known_ids: HashSet<String> = history.iter().map(|m| m.id.clone()).collect();

        let session = Arc::new(Mutex::new(Self {
            conversation_id: conversation_id.to_string(),
            message_count,
            preview,
            known_ids,
            transcript_dirty: reconciled,
        }));

        if reconciled {
            if let Err(e) = crate::conversation_session::sync_ordered(conversation_id, history) {
                log::warn!(
                    "conversation_transcript: sync after reconcile failed conversation_id={conversation_id}: {e:#}"
                );
            } else {
                let mut s = session.lock();
                s.transcript_dirty = false;
                s.message_count = meta_message_count(conversation_id, history.len() as u32);
                s.preview = conversation_preview(history);
            }
        } else {
            crate::conversation_session::publish_working_set(
                conversation_id,
                history,
                Some(message_count),
            );
            if let Ok(store) = conversation_store::global_store() {
                let s = session.lock();
                if let Err(e) =
                    store.flush_conversation_meta(conversation_id, s.message_count, &s.preview)
                {
                    log::warn!(
                        "conversation_transcript: flush_meta after bootstrap failed conversation_id={conversation_id}: {e:#}"
                    );
                }
            }
        }

        global_registry().register(conversation_id, session.clone());
        log::debug!(
            "conversation_transcript: begin conversation_id={conversation_id} db_messages={message_count} working_history={}",
            history.len()
        );
        Ok(session)
    }

    pub fn end(history: &[ChatMessage], session: &Arc<Mutex<Self>>) {
        let conversation_id = {
            let mut s = session.lock();
            s.message_count = meta_message_count(&s.conversation_id, history.len() as u32);
            s.preview = conversation_preview(history);
            s.conversation_id.clone()
        };
        if let Err(e) =
            ConversationTranscriptSession::flush_transcript(&conversation_id, history, session)
        {
            log::warn!(
                "conversation_transcript: end flush failed conversation_id={conversation_id}: {e:#}"
            );
        }
        crate::conversation_session::publish_working_set(
            &conversation_id,
            history,
            Some(meta_message_count(&conversation_id, history.len() as u32)),
        );
        global_registry().unregister(&conversation_id);
        log::info!(
            "conversation_transcript: end conversation_id={conversation_id} working_history={} db_messages={}",
            history.len(),
            meta_message_count(&conversation_id, history.len() as u32)
        );
    }

    /// Insert or update a tool result row in `history` (memory only until flush).
    pub fn record_tool_result(
        session: &Arc<Mutex<Self>>,
        history: &mut Vec<ChatMessage>,
        hint_message_id: &str,
        tool_call_id: &str,
        content: &str,
    ) {
        let mut msg = reconcile::tool_result_message(tool_call_id, content);
        msg.tool_name = reconcile::tool_name_for_call(history, tool_call_id);
        if let Some(idx) =
            reconcile::find_existing_tool_index(history, tool_call_id, hint_message_id)
        {
            history[idx].content = content.to_string();
            if history[idx].tool_name.is_none() {
                history[idx].tool_name = msg.tool_name.clone();
            }
            let id = history[idx].id.clone();
            session.lock().known_ids.insert(id);
        } else if let Some(idx) =
            reconcile::find_tool_insert_index(history, tool_call_id, hint_message_id)
        {
            history.insert(idx, msg);
            session.lock().known_ids.insert(history[idx].id.clone());
        } else {
            log::warn!(
                "conversation_transcript: dropped tool result (no anchor assistant) tool_call_id={tool_call_id} hint_message_id={hint_message_id}"
            );
            return;
        }
        let mut s = session.lock();
        s.transcript_dirty = true;
        s.message_count = meta_message_count(&s.conversation_id, history.len() as u32);
    }

    /// Upsert one message row without full-transcript reload.
    pub fn upsert_message(session: &Arc<Mutex<Self>>, msg: &ChatMessage) {
        let mut s = session.lock();
        let is_new = !s.known_ids.contains(&msg.id);
        if is_new {
            s.known_ids.insert(msg.id.clone());
            s.message_count = s.message_count.saturating_add(1);
        }
        reconcile::maybe_update_preview(&mut s.preview, msg);
        let conversation_id = s.conversation_id.clone();
        if let Err(e) = crate::conversation_session::upsert_message(&conversation_id, msg) {
            log::warn!(
                "conversation_transcript: upsert_message failed conversation_id={conversation_id} message_id={}: {e:#}",
                msg.id
            );
        } else {
            s.message_count = meta_message_count(&conversation_id, s.message_count);
        }
    }

    pub fn mark_transcript_dirty(session: &Arc<Mutex<Self>>, history: &[ChatMessage]) {
        let mut s = session.lock();
        s.transcript_dirty = true;
        s.message_count = meta_message_count(&s.conversation_id, history.len() as u32);
        s.preview = conversation_preview(history);
    }

    pub fn flush_transcript(
        conversation_id: &str,
        history: &[ChatMessage],
        session: &Arc<Mutex<Self>>,
    ) -> Result<()> {
        let mut s = session.lock();
        s.preview = conversation_preview(history);
        s.message_count = meta_message_count(conversation_id, history.len() as u32);
        if !s.transcript_dirty {
            return Ok(());
        }
        drop(s);
        crate::conversation_session::sync_ordered(conversation_id, history)?;
        let mut s = session.lock();
        s.transcript_dirty = false;
        s.message_count = meta_message_count(conversation_id, history.len() as u32);
        s.preview = conversation_preview(history);
        s.known_ids = history.iter().map(|m| m.id.clone()).collect();
        Ok(())
    }
}

/// Route transcript writes through an active session when present.
pub fn upsert_message(conversation_id: &str, msg: &ChatMessage) {
    if let Some(session) = global_registry().get(conversation_id) {
        ConversationTranscriptSession::upsert_message(&session, msg);
        return;
    }
    log::warn!(
        "conversation_transcript: upsert without active session conversation_id={conversation_id} message_id={}",
        msg.id
    );
    if let Err(e) = crate::conversation_session::upsert_message(conversation_id, msg) {
        log::warn!(
            "conversation_transcript: fallback upsert failed conversation_id={conversation_id} message_id={}: {e:#}",
            msg.id
        );
    }
}

/// Insert or update a tool row in an in-memory history only (no DB / transcript session).
pub fn insert_tool_result_in_history(
    history: &mut Vec<ChatMessage>,
    hint_message_id: &str,
    tool_call_id: &str,
    content: &str,
) {
    let mut msg = reconcile::tool_result_message(tool_call_id, content);
    msg.tool_name = reconcile::tool_name_for_call(history, tool_call_id);
    if let Some(idx) = reconcile::find_existing_tool_index(history, tool_call_id, hint_message_id) {
        history[idx].content = content.to_string();
        if history[idx].tool_name.is_none() {
            history[idx].tool_name = msg.tool_name.clone();
        }
    } else if let Some(idx) =
        reconcile::find_tool_insert_index(history, tool_call_id, hint_message_id)
    {
        history.insert(idx, msg);
    } else {
        log::warn!(
            "conversation_transcript: dropped in-memory tool result tool_call_id={tool_call_id} hint_message_id={hint_message_id}"
        );
    }
}

pub fn record_tool_result(
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    hint_message_id: &str,
    tool_call_id: &str,
    content: &str,
) {
    if let Some(session) = global_registry().get(conversation_id) {
        ConversationTranscriptSession::record_tool_result(
            &session,
            history,
            hint_message_id,
            tool_call_id,
            content,
        );
        return;
    }
    // Fallback when no active session (tests / edge paths).
    insert_tool_result_in_history(history, hint_message_id, tool_call_id, content);
}

pub fn flush_after_tool_pass(conversation_id: &str, history: &[ChatMessage]) {
    if let Some(session) = global_registry().get(conversation_id) {
        if let Err(e) =
            ConversationTranscriptSession::flush_transcript(conversation_id, history, &session)
        {
            log::warn!(
                "conversation_transcript: flush_after_tool_pass failed conversation_id={conversation_id}: {e:#}"
            );
        }
    }
}

pub fn sync_ordered(conversation_id: &str, history: &[ChatMessage]) {
    if let Err(e) = crate::conversation_session::sync_ordered(conversation_id, history) {
        log::warn!(
            "conversation_transcript: sync_ordered failed conversation_id={conversation_id}: {e:#}"
        );
    }
    let count = meta_message_count(conversation_id, history.len() as u32);
    let preview = conversation_preview(history);
    if let Some(session) = global_registry().get(conversation_id) {
        let mut s = session.lock();
        s.transcript_dirty = false;
        s.message_count = count;
        s.preview = preview;
        s.known_ids = history.iter().map(|m| m.id.clone()).collect();
    }
}

/// Persist compression splice: exclude payloads + suffix position shift + summary insert.
/// Call with the full in-memory list (prefix already marked, summary already inserted)
/// *before* draining the excluded prefix.
pub fn persist_compression_splice(
    conversation_id: &str,
    excluded_messages: &[ChatMessage],
    summary: &ChatMessage,
    insert_before_message_id: &str,
    preview: &str,
) {
    if let Err(e) = crate::conversation_session::persist_compression_splice(
        conversation_id,
        excluded_messages,
        summary,
        insert_before_message_id,
        preview,
    ) {
        log::warn!(
            "conversation_transcript: persist_compression_splice failed conversation_id={conversation_id}: {e:#}"
        );
        return;
    }
    if let Ok(store) = conversation_store::global_store() {
        if let Ok(count) = store.message_count(conversation_id) {
            if let Some(session) = global_registry().get(conversation_id) {
                let mut s = session.lock();
                s.transcript_dirty = false;
                s.message_count = count;
                s.preview = preview.to_string();
                s.known_ids.insert(summary.id.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ExcludedReason, MessageContextState, Role, ToolCall};

    fn assistant_with_call(id: &str, call_id: &str) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            role: Role::Assistant,
            content: String::new(),
            status: "done".into(),
            created_at: 0,
            tool_calls: Some(vec![ToolCall {
                id: call_id.into(),
                name: "terminal".into(),
                arguments: "{}".into(),
                status: "running".into(),
                result: None,
                error: None,
                duration_ms: None,
                risk_level: None,
                display_label: None,
                display_summary: None,
            }]),
            tool_call_id: None,
            tool_name: None,
            error_message: None,
            reasoning: None,
            thoughts: None,
            headline: None,
            raw_content: None,
            tool_raw_output: None,
            agent_id: None,
            agent_instance_id: None,
            agent_name: None,
            agent_trace: None,
            image_slot_labels: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
            attachments: None,
            anchor_message_id: None,
            trace_id: None,
            task_id: None,
            spawn_depth: None,
            agent_chain: None,
        }
    }

    #[test]
    fn tool_inserts_after_anchor_assistant() {
        let mut history = vec![
            ChatMessage {
                id: "u1".into(),
                role: Role::User,
                content: "hi".into(),
                status: "done".into(),
                created_at: 0,
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
                error_message: None,
                reasoning: None,
                thoughts: None,
                headline: None,
                raw_content: None,
                tool_raw_output: None,
                agent_id: None,
                agent_instance_id: None,
                agent_name: None,
                agent_trace: None,
                image_slot_labels: None,
                images_base64: None,
                computer_round_screen_rel_path: None,
                ui_bindings: None,
                context_state: None,
                attachments: None,
                anchor_message_id: None,
                trace_id: None,
                task_id: None,
                spawn_depth: None,
                agent_chain: None,
            },
            assistant_with_call("a1", "call_a"),
        ];
        record_tool_result("c1", &mut history, "a1", "call_a", "{\"ok\":true}");
        assert_eq!(history.len(), 3);
        assert!(matches!(history[2].role, Role::Tool));
        assert_eq!(history[2].tool_call_id.as_deref(), Some("call_a"));
    }

    #[test]
    fn reconcile_removes_orphan_after_user() {
        let mut history = vec![
            ChatMessage {
                id: "u1".into(),
                role: Role::User,
                content: "q".into(),
                status: "done".into(),
                created_at: 0,
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
                error_message: None,
                reasoning: None,
                thoughts: None,
                headline: None,
                raw_content: None,
                tool_raw_output: None,
                agent_id: None,
                agent_instance_id: None,
                agent_name: None,
                agent_trace: None,
                image_slot_labels: None,
                images_base64: None,
                computer_round_screen_rel_path: None,
                ui_bindings: None,
                context_state: None,
                attachments: None,
                anchor_message_id: None,
                trace_id: None,
                task_id: None,
                spawn_depth: None,
                agent_chain: None,
            },
            reconcile::tool_result_message("call_orphan", "stale"),
        ];
        let changed = reconcile::reconcile_tool_messages(&mut history);
        assert!(changed);
        assert_eq!(history.len(), 1);
    }

    fn plain_user(id: &str, content: &str) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            role: Role::User,
            content: content.into(),
            status: "done".into(),
            created_at: 0,
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
            error_message: None,
            reasoning: None,
            thoughts: None,
            headline: None,
            raw_content: None,
            tool_raw_output: None,
            agent_id: None,
            agent_instance_id: None,
            agent_name: None,
            agent_trace: None,
            image_slot_labels: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
            attachments: None,
            anchor_message_id: None,
            trace_id: None,
            task_id: None,
            spawn_depth: None,
            agent_chain: None,
        }
    }

    #[test]
    fn filter_lead_working_history_drops_soft_excluded() {
        let mut excluded = plain_user("old", "compressed away");
        excluded.context_state = Some(MessageContextState {
            included: false,
            excluded_reason: Some(ExcludedReason::ContextCompression),
        });
        let kept = plain_user("new", "still in context");
        let working = filter_lead_working_history(vec![excluded, kept]);
        assert_eq!(working.len(), 1);
        assert_eq!(working[0].id, "new");
    }
}
