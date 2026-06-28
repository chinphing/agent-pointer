//! Webhook ingress message normalization (OpenClaw `/hooks/agent` alignment).
//!
//! OpenClaw accepts a single required `message` string per hook turn. Pointer
//! also supports `text` and a full `messages` array for advanced callers.
//!
//! Session continuity: like cron scheduler (and OpenClaw `sessionMode:
//! persistent`), the default path loads the active webhook session transcript
//! and appends the new user turn — so multiple ingress calls on the same day
//! share context within `webhook:{src}:{yyyymmdd}`.

use anyhow::{bail, Result};

use crate::conversation_store::ConversationStore;
use crate::models::{ChatMessage, Role};

/// Inbound turn fields from `POST /api/webhooks/:src` (camelCase at the HTTP boundary).
#[derive(Debug, Clone, Default)]
pub struct WebhookInboundTurn {
    pub text: Option<String>,
    /// OpenClaw `/hooks/agent` field name; treated as alias of `text`.
    pub message: Option<String>,
    /// Optional label prefix (OpenClaw `name`), e.g. `"GitHub"` → `[GitHub] …`.
    pub name: Option<String>,
    pub messages: Option<Vec<ChatMessage>>,
}

/// Build the message history passed to `RunDispatcher` for one webhook ingress.
///
/// - **Append mode (default):** load stored transcript for `conversation_id`, append
///   the new user turn (`text`, `message`, or a single user entry in `messages`).
/// - **Full history mode:** when `messages` contains assistant/tool rows or more
///   than one entry, use the array as-is (advanced / HTTP Runs-style callers).
pub fn build_webhook_dispatch_messages(
    store: &ConversationStore,
    conversation_id: &str,
    inbound: &WebhookInboundTurn,
) -> Result<Vec<ChatMessage>> {
    if let Some(msgs) = inbound.messages.as_ref().filter(|m| !m.is_empty()) {
        if is_full_history_override(msgs) {
            return Ok(msgs.clone());
        }
        let mut history = store.load_messages(conversation_id).unwrap_or_default();
        history.extend(msgs.clone());
        return Ok(history);
    }

    let raw = resolve_inbound_text(inbound)?;
    let content = apply_name_prefix(inbound.name.as_deref(), &raw);
    let user_msg = ChatMessage::user_text(content);
    let mut history = store.load_messages(conversation_id).unwrap_or_default();
    history.push(user_msg);
    Ok(history)
}

/// Last user message in `messages` (for UI `InjectedUserMessage` broadcast).
pub fn last_inbound_user_message(messages: &[ChatMessage]) -> Option<&ChatMessage> {
    messages.iter().rev().find(|m| matches!(m.role, Role::User))
}

fn resolve_inbound_text(inbound: &WebhookInboundTurn) -> Result<String> {
    let text = inbound
        .text
        .as_deref()
        .or(inbound.message.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match text {
        Some(s) => Ok(s.to_string()),
        None => bail!("webhook body must contain `text`, `message`, or `messages`"),
    }
}

fn apply_name_prefix(name: Option<&str>, text: &str) -> String {
    let label = name.map(str::trim).filter(|s| !s.is_empty());
    match label {
        Some(n) => format!("[{n}] {text}"),
        None => text.to_string(),
    }
}

fn is_full_history_override(messages: &[ChatMessage]) -> bool {
    messages.len() > 1
        || messages
            .iter()
            .any(|m| !matches!(m.role, Role::User))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_store::ConversationStore;

    fn store() -> ConversationStore {
        let dir = std::env::temp_dir().join(format!(
            "pointer-webhook-ingress-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        ConversationStore::open(dir.join("conversations.db")).unwrap()
    }

    #[test]
    fn append_mode_loads_history_and_new_text() {
        let s = store();
        let conv = "webhook:github:20260628";
        s.append_missing_messages(conv, &[ChatMessage::user_text("prior")])
            .unwrap();
        let msgs = build_webhook_dispatch_messages(
            &s,
            conv,
            &WebhookInboundTurn {
                text: Some("new event".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "prior");
        assert_eq!(msgs[1].content, "new event");
    }

    #[test]
    fn openclaw_message_alias_and_name_prefix() {
        let s = store();
        let conv = "webhook:codeup:20260628";
        let msgs = build_webhook_dispatch_messages(
            &s,
            conv,
            &WebhookInboundTurn {
                message: Some("push received".into()),
                name: Some("Codeup".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "[Codeup] push received");
    }

    #[test]
    fn full_history_when_messages_include_assistant() {
        let s = store();
        let conv = "webhook:x:20260628";
        s.append_missing_messages(conv, &[ChatMessage::user_text("old")])
            .unwrap();
        let mut assistant = ChatMessage::user_text("a1");
        assistant.role = Role::Assistant;
        let override_msgs = vec![ChatMessage::user_text("u1"), assistant];
        let msgs = build_webhook_dispatch_messages(
            &s,
            conv,
            &WebhookInboundTurn {
                messages: Some(override_msgs),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(msgs.len(), 2);
        assert!(!matches!(msgs[1].role, Role::User));
    }
}
