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
    let Some(store) = store() else {
        return;
    };
    if let Err(e) = store.append_missing_messages(conversation_id, history) {
        log::warn!(
            "conversation_persist: append_missing failed conversation_id={conversation_id}: {e:#}"
        );
    }
}

pub fn upsert_message(conversation_id: &str, msg: &ChatMessage) {
    let Some(store) = store() else {
        return;
    };
    if let Err(e) = store.upsert_message(conversation_id, msg) {
        log::warn!(
            "conversation_persist: upsert_message failed conversation_id={conversation_id} message_id={}: {e:#}",
            msg.id
        );
    }
}

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

pub fn patch_tool_rounds(
    conversation_id: &str,
    tool_rounds_used: u32,
    tool_rounds_used_supervisor: u32,
    updated_at_ms: i64,
) {
    let Some(store) = store() else {
        return;
    };
    let Ok(list) = store.load_all() else {
        return;
    };
    let Some(conv) = list.into_iter().find(|c| c.id == conversation_id) else {
        log::warn!(
            "conversation_persist: patch_tool_rounds missing conversation_id={conversation_id}"
        );
        return;
    };
    let mut meta = ConversationMeta::from(&conv);
    meta.tool_rounds_used = tool_rounds_used;
    meta.tool_rounds_used_supervisor = tool_rounds_used_supervisor;
    meta.updated_at = updated_at_ms;
    upsert_meta(&meta);
}
