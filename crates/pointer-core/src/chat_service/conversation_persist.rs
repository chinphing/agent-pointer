//! Best-effort conversation DB writes from the chat loop (P0 / P2).

use crate::conversation_store;
use crate::models::{ChatMessage, ConversationMeta};

fn store() -> Option<std::sync::Arc<conversation_store::ConversationStore>> {
    match conversation_store::global_store() {
        Ok(s) => Some(s),
        Err(e) => {
            log::warn!("conversation_persist: store unavailable: {e:#}");
            None
        }
    }
}

pub fn append_missing(conversation_id: &str, history: &[ChatMessage]) {
    if let Err(e) = crate::conversation_session::append_missing(conversation_id, history) {
        log::warn!(
            "conversation_persist: append_missing failed conversation_id={conversation_id}: {e:#}"
        );
    }
}

pub fn upsert_message(conversation_id: &str, msg: &ChatMessage) {
    crate::conversation_transcript::upsert_message(conversation_id, msg);
}

pub fn upsert_meta(meta: &ConversationMeta) {
    let Some(store) = store() else {
        return;
    };
    if let Err(e) = store.upsert_meta(meta) {
        log::warn!(
            "conversation_persist: upsert_meta failed conversation_id={}: {e:#}",
            meta.id
        );
    }
}

/// Persist an auto-created session sandbox path so later chat runs reuse it.
pub fn patch_ephemeral_workspace(conversation_id: &str, workspace_root: &str) {
    let trimmed = workspace_root.trim();
    if trimmed.is_empty() {
        return;
    }
    let Some(store) = store() else {
        return;
    };
    // Read only this conversation's meta row (O(log n) via PK). The previous
    // implementation called store.load_all(), which deserialized every
    // conversation and every message on every chat send — catastrophic at scale.
    let Ok(Some(conv)) = store.load_meta(conversation_id) else {
        log::warn!(
            "conversation_persist: patch_ephemeral_workspace missing conversation_id={conversation_id}"
        );
        return;
    };
    if conv.workspace_root.trim() == trimmed {
        return;
    }
    let mut meta = conv;
    meta.workspace_root = trimmed.to_string();
    meta.workspace_user_set = false;
    meta.workspace_inherit_disabled = true;
    meta.updated_at = chrono::Utc::now().timestamp_millis();
    upsert_meta(&meta);
}

pub fn patch_tool_rounds(
    conversation_id: &str,
    tool_rounds_used: u32,
    tool_rounds_used_supervisor: u32,
    updated_at_ms: i64,
) {
    let Some(store) = store() else {
        return;
    };
    // Read only this conversation's meta row (O(log n) via PK); never load_all.
    let Ok(Some(conv)) = store.load_meta(conversation_id) else {
        log::warn!(
            "conversation_persist: patch_tool_rounds missing conversation_id={conversation_id}"
        );
        return;
    };
    let mut meta = conv;
    meta.tool_rounds_used = tool_rounds_used;
    meta.tool_rounds_used_supervisor = tool_rounds_used_supervisor;
    meta.updated_at = updated_at_ms;
    upsert_meta(&meta);
}
