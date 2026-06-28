//! Helpers for webhook blocking responses (OpenClaw `blocking: true` alignment).

use crate::conversation_store::ConversationStore;
use crate::models::Role;
use anyhow::Result;

/// Last non-empty assistant message content in a conversation transcript.
pub fn last_assistant_text(store: &ConversationStore, conversation_id: &str) -> Result<Option<String>> {
    let messages = store.load_messages(conversation_id)?;
    for msg in messages.iter().rev() {
        if !matches!(msg.role, Role::Assistant) {
            continue;
        }
        let trimmed = msg.content.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_string()));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_store::ConversationStore;
    use crate::models::ChatMessage;

    fn store() -> ConversationStore {
        let dir = std::env::temp_dir().join(format!(
            "pointer-webhook-result-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        ConversationStore::open(dir.join("conversations.db")).unwrap()
    }

    #[test]
    fn last_assistant_skips_empty_and_tool_roles() {
        let s = store();
        let conv = "webhook:test";
        let mut empty = ChatMessage::user_text("placeholder");
        empty.id = "a1".into();
        empty.role = Role::Assistant;
        empty.content = "  ".into();
        let mut final_msg = ChatMessage::user_text("placeholder");
        final_msg.id = "a2".into();
        final_msg.role = Role::Assistant;
        final_msg.content = "final answer".into();
        s.append_missing_messages(conv, &[ChatMessage::user_text("hi"), empty, final_msg])
            .unwrap();
        assert_eq!(
            last_assistant_text(&s, conv).unwrap().as_deref(),
            Some("final answer")
        );
    }
}
