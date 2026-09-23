//! Helpers for webhook blocking responses and async run polling.

use crate::conversation_store::ConversationStore;
use crate::dispatcher::trigger::{TriggerMeta, TriggerSource};
use anyhow::Result;
use serde::Serialize;

/// Poll response for `GET /api/webhooks/:src/runs/:runId`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookRunView {
    pub run_id: String,
    pub status: String,
    pub conversation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Last non-empty assistant message content in a conversation transcript.
pub fn last_assistant_text(
    store: &ConversationStore,
    conversation_id: &str,
) -> Result<Option<String>> {
    store.load_last_assistant_content(conversation_id)
}

/// Load a webhook run for polling when it belongs to `:src`.
pub fn webhook_run_view_for_source(
    store: &ConversationStore,
    src: &str,
    run_id: &str,
) -> Result<Option<WebhookRunView>> {
    let Some(record) = store.runs_get(run_id)? else {
        return Ok(None);
    };
    if record.trigger_source != TriggerSource::Webhook.as_str() {
        return Ok(None);
    }
    let meta: TriggerMeta = serde_json::from_str(&record.trigger_meta_json).unwrap_or_default();
    let Some(webhook_source) = meta.webhook_source else {
        return Ok(None);
    };
    if webhook_source != src {
        return Ok(None);
    }

    let terminal = matches!(record.status.as_str(), "finished" | "failed" | "cancelled");
    let text = if record.status == "finished" {
        last_assistant_text(store, &record.conversation_id)?
    } else {
        None
    };
    let error = if record.status == "failed" || record.status == "cancelled" {
        record.error.clone()
    } else {
        None
    };

    Ok(Some(WebhookRunView {
        run_id: record.run_id,
        status: record.status,
        conversation_id: record.conversation_id,
        text: if terminal { text } else { None },
        error: if terminal { error } else { None },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_store::ConversationStore;
    use crate::models::{ChatMessage, Role};

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

    #[test]
    fn webhook_run_view_rejects_foreign_source() {
        let s = store();
        let meta = TriggerMeta {
            webhook_source: Some("other".into()),
            ..TriggerMeta::empty()
        };
        s.runs_insert_queued(
            "run-1",
            "webhook:ci:20260708",
            TriggerSource::Webhook,
            &serde_json::to_string(&meta).unwrap(),
            None,
        )
        .unwrap();
        assert!(webhook_run_view_for_source(&s, "ci", "run-1")
            .unwrap()
            .is_none());
    }
}
