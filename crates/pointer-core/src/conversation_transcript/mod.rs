//! Single entry for in-flight conversation transcript mutations during `run_chat`.
//!
//! Canonical on disk: `role: tool` rows immediately after the assistant that issued
//! `tool_calls`. In-memory `history` is the source of order; DB is synced in batches.

mod registry;
mod reconcile;

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

/// Active transcript session for one `run_chat` invocation.
pub struct ConversationTranscriptSession {
    conversation_id: String,
    message_count: u32,
    preview: String,
    known_ids: HashSet<String>,
    transcript_dirty: bool,
}

impl ConversationTranscriptSession {
    pub fn begin(conversation_id: &str, history: &mut Vec<ChatMessage>) -> Result<Arc<Mutex<Self>>> {
        let reconciled = reconcile::reconcile_tool_messages(history);
        let message_count = history.len() as u32;
        let preview = conversation_preview(history);
        let known_ids: HashSet<String> = history.iter().map(|m| m.id.clone()).collect();

        let session = Arc::new(Mutex::new(Self {
            conversation_id: conversation_id.to_string(),
            message_count,
            preview,
            known_ids,
            transcript_dirty: reconciled,
        }));

        if let Ok(store) = conversation_store::global_store() {
            if let Err(e) = store.append_missing_messages(conversation_id, history) {
                log::warn!(
                    "conversation_transcript: append_missing failed conversation_id={conversation_id}: {e:#}"
                );
            }
            let mut s = session.lock();
            s.message_count = history.len() as u32;
            s.preview = conversation_preview(history);
            if s.transcript_dirty {
                if let Err(e) = store.sync_messages_ordered_with_meta(
                    conversation_id,
                    history,
                    s.message_count,
                    &s.preview,
                ) {
                    log::warn!(
                        "conversation_transcript: sync after reconcile failed conversation_id={conversation_id}: {e:#}"
                    );
                } else {
                    s.transcript_dirty = false;
                }
            }
            if let Err(e) = store.flush_conversation_meta(
                conversation_id,
                s.message_count,
                &s.preview,
            ) {
                log::warn!(
                    "conversation_transcript: flush_meta after bootstrap failed conversation_id={conversation_id}: {e:#}"
                );
            }
        }

        global_registry().register(conversation_id, session.clone());
        log::debug!(
            "conversation_transcript: begin conversation_id={conversation_id} messages={message_count}"
        );
        Ok(session)
    }

    pub fn end(history: &[ChatMessage], session: &Arc<Mutex<Self>>) {
        let conversation_id = {
            let mut s = session.lock();
            s.message_count = history.len() as u32;
            s.preview = conversation_preview(history);
            s.conversation_id.clone()
        };
        if let Err(e) = ConversationTranscriptSession::flush_transcript(&conversation_id, history, session) {
            log::warn!(
                "conversation_transcript: end flush failed conversation_id={conversation_id}: {e:#}"
            );
        }
        global_registry().unregister(&conversation_id);
        log::info!(
            "conversation_transcript: end conversation_id={conversation_id} messages={}",
            history.len()
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
        let msg = reconcile::tool_result_message(tool_call_id, content);
        if let Some(idx) =
            reconcile::find_existing_tool_index(history, tool_call_id, hint_message_id)
        {
            history[idx].content = content.to_string();
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
        s.message_count = history.len() as u32;
        s.transcript_dirty = true;
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
        let count = s.message_count;
        let preview = s.preview.clone();
        if let Ok(store) = conversation_store::global_store() {
            if let Err(e) = store.upsert_message_no_refresh(&conversation_id, msg) {
                log::warn!(
                    "conversation_transcript: upsert_message failed conversation_id={conversation_id} message_id={}: {e:#}",
                    msg.id
                );
            } else if let Err(e) = store.flush_conversation_meta(&conversation_id, count, &preview) {
                log::warn!(
                    "conversation_transcript: flush_meta after upsert failed conversation_id={conversation_id}: {e:#}"
                );
            }
        }
    }

    pub fn mark_transcript_dirty(session: &Arc<Mutex<Self>>, history: &[ChatMessage]) {
        let mut s = session.lock();
        s.transcript_dirty = true;
        s.message_count = history.len() as u32;
        s.preview = conversation_preview(history);
    }

    pub fn flush_transcript(
        conversation_id: &str,
        history: &[ChatMessage],
        session: &Arc<Mutex<Self>>,
    ) -> Result<()> {
        let mut s = session.lock();
        s.message_count = history.len() as u32;
        s.preview = conversation_preview(history);
        if !s.transcript_dirty {
            return Ok(());
        }
        let store = conversation_store::global_store()?;
        store.sync_messages_ordered_with_meta(conversation_id, history, s.message_count, &s.preview)?;
        s.transcript_dirty = false;
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
    if let Ok(store) = conversation_store::global_store() {
        if let Err(e) = store.upsert_message_no_refresh(conversation_id, msg) {
            log::warn!(
                "conversation_transcript: fallback upsert failed conversation_id={conversation_id} message_id={}: {e:#}",
                msg.id
            );
            return;
        }
        let count = store.message_count(conversation_id).unwrap_or(0);
        let preview = match store.stored_conversation_preview(conversation_id) {
            Ok(p) if !p.is_empty() => p,
            _ => conversation_preview(&[msg.clone()]),
        };
        if let Err(e) = store.flush_conversation_meta(conversation_id, count, &preview) {
            log::warn!(
                "conversation_transcript: fallback flush_meta failed conversation_id={conversation_id}: {e:#}"
            );
        }
    }
}

/// Insert or update a tool row in an in-memory history only (no DB / transcript session).
pub fn insert_tool_result_in_history(
    history: &mut Vec<ChatMessage>,
    hint_message_id: &str,
    tool_call_id: &str,
    content: &str,
) {
    let msg = reconcile::tool_result_message(tool_call_id, content);
    if let Some(idx) =
        reconcile::find_existing_tool_index(history, tool_call_id, hint_message_id)
    {
        history[idx].content = content.to_string();
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
    let count = history.len() as u32;
    let preview = conversation_preview(history);
    if let Ok(store) = conversation_store::global_store() {
        if let Err(e) = store.sync_messages_ordered_with_meta(conversation_id, history, count, &preview) {
            log::warn!(
                "conversation_transcript: sync_ordered failed conversation_id={conversation_id}: {e:#}"
            );
        }
    }
    if let Some(session) = global_registry().get(conversation_id) {
        let mut s = session.lock();
        s.transcript_dirty = false;
        s.message_count = count;
        s.preview = preview;
        s.known_ids = history.iter().map(|m| m.id.clone()).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Role, ToolCall};

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
            },
            reconcile::tool_result_message("call_orphan", "stale"),
        ];
        let changed = reconcile::reconcile_tool_messages(&mut history);
        assert!(changed);
        assert_eq!(history.len(), 1);
    }
}
