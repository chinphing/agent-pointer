//! Webhook ingress message normalization.
//!
//! Primary path: Pointer / OpenClaw-style fields (`text`, `message`, `messages`).
//! Fallback: when no structured message is present, the entire request body
//! becomes the user turn (JSON compact string or plain text). This lets third
//! parties (GitHub, Codeup, …) POST native payloads without mapping config.

use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::conversation_store::ConversationStore;
use crate::models::{ChatMessage, MediaAttachment, Role};
use crate::webhook_attachment::validate_webhook_attachments;

/// Max raw body size for webhook ingress (including raw-body fallback).
pub const MAX_WEBHOOK_BODY_BYTES: usize = 256 * 1024;

/// JSON body for `POST /api/webhooks/:src` (camelCase at the HTTP boundary).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookIngressBody {
    pub conversation_id: Option<String>,
    pub text: Option<String>,
    /// OpenClaw `/hooks/agent` alias for `text`.
    pub message: Option<String>,
    /// Optional label prefix (OpenClaw `name`), e.g. `"GitHub"` → `[GitHub] …`.
    pub name: Option<String>,
    pub messages: Option<Vec<ChatMessage>>,
    /// User attachments for the inbound turn (`text` / `message` path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<MediaAttachment>>,
    pub agent_mode: Option<String>,
    pub lead_agent_id: Option<String>,
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub enabled_skill_ids: Vec<String>,
    #[serde(default)]
    pub workspace_root: String,
    #[serde(default)]
    pub blocking: bool,
    pub timeout_seconds: Option<u64>,
}

/// Parsed webhook request ready for dispatch.
#[derive(Debug, Clone)]
pub struct WebhookIngressPayload {
    pub body: WebhookIngressBody,
    pub inbound: WebhookInboundTurn,
    /// True when the user turn was synthesized from the full raw body.
    pub used_raw_body_fallback: bool,
}

/// Inbound turn fields passed to transcript assembly.
#[derive(Debug, Clone, Default)]
pub struct WebhookInboundTurn {
    pub text: Option<String>,
    pub message: Option<String>,
    pub name: Option<String>,
    pub messages: Option<Vec<ChatMessage>>,
    pub attachments: Option<Vec<MediaAttachment>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebhookParseError {
    PayloadTooLarge { max: usize },
    EmptyBody,
    InvalidJson(String),
    MessageBuild(String),
}

impl std::fmt::Display for WebhookParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PayloadTooLarge { max } => {
                write!(f, "webhook payload too large (max {max} bytes)")
            }
            Self::EmptyBody => write!(f, "webhook body must not be empty"),
            Self::InvalidJson(msg) => write!(f, "invalid webhook json: {msg}"),
            Self::MessageBuild(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for WebhookParseError {}

/// Parse raw HTTP body into control fields + inbound turn (with optional fallback).
pub fn parse_webhook_body(raw: &[u8], src: &str) -> Result<WebhookIngressPayload, WebhookParseError> {
    if raw.len() > MAX_WEBHOOK_BODY_BYTES {
        return Err(WebhookParseError::PayloadTooLarge {
            max: MAX_WEBHOOK_BODY_BYTES,
        });
    }
    if raw.is_empty() {
        return Err(WebhookParseError::EmptyBody);
    }

    if let Ok(value) = serde_json::from_slice::<Value>(raw) {
        return parse_json_webhook_body(&value, src);
    }

    let text = std::str::from_utf8(raw)
        .ok()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(WebhookParseError::EmptyBody)?;
    let name = Some(src.to_string());
    Ok(WebhookIngressPayload {
        body: WebhookIngressBody::default(),
        inbound: WebhookInboundTurn {
            text: Some(text.to_string()),
            message: None,
            name,
            messages: None,
            attachments: None,
        },
        used_raw_body_fallback: true,
    })
}

fn parse_json_webhook_body(value: &Value, src: &str) -> Result<WebhookIngressPayload, WebhookParseError> {
    let body: WebhookIngressBody = serde_json::from_value(value.clone()).map_err(|e| {
        WebhookParseError::InvalidJson(e.to_string())
    })?;

    if has_structured_message(&body) {
        return Ok(WebhookIngressPayload {
            inbound: WebhookInboundTurn {
                text: body.text.clone(),
                message: body.message.clone(),
                name: body.name.clone(),
                messages: body.messages.clone(),
                attachments: body.attachments.clone(),
            },
            body,
            used_raw_body_fallback: false,
        });
    }

    let raw_msg = serde_json::to_string(value).map_err(|e| {
        WebhookParseError::MessageBuild(format!("webhook fallback serialize failed: {e}"))
    })?;
    if raw_msg.trim().is_empty() {
        return Err(WebhookParseError::EmptyBody);
    }
    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| Some(src.to_string()));

    Ok(WebhookIngressPayload {
        inbound: WebhookInboundTurn {
            text: Some(raw_msg),
            message: None,
            name,
            messages: None,
            attachments: None,
        },
        body,
        used_raw_body_fallback: true,
    })
}

fn has_structured_message(body: &WebhookIngressBody) -> bool {
    body.text
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
        || body
            .message
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
        || body
            .messages
            .as_ref()
            .is_some_and(|messages| !messages.is_empty())
        || body
            .attachments
            .as_ref()
            .is_some_and(|attachments| !attachments.is_empty())
}

/// Build the message history passed to `RunDispatcher` for one webhook ingress.
pub fn build_webhook_dispatch_messages(
    store: &ConversationStore,
    conversation_id: &str,
    inbound: &WebhookInboundTurn,
) -> Result<Vec<ChatMessage>> {
    if let Some(msgs) = inbound.messages.as_ref().filter(|m| !m.is_empty()) {
        for msg in msgs {
            if let Some(atts) = msg.attachments.as_ref().filter(|a| !a.is_empty()) {
                validate_webhook_attachments(conversation_id, atts)?;
            }
        }
        if is_full_history_override(msgs) {
            return Ok(msgs.clone());
        }
        let mut history = store.load_messages(conversation_id).unwrap_or_default();
        history.extend(msgs.clone());
        return Ok(history);
    }

    let raw = resolve_inbound_text(inbound)?;
    let content = apply_name_prefix(inbound.name.as_deref(), &raw);
    let mut user_msg = ChatMessage::user_text(content);
    if let Some(atts) = inbound.attachments.as_ref().filter(|a| !a.is_empty()) {
        validate_webhook_attachments(conversation_id, atts)?;
        user_msg.attachments = Some(atts.clone());
    }
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
        .map(str::trim);
    if let Some(s) = text.filter(|s| !s.is_empty()) {
        return Ok(s.to_string());
    }
    if inbound
        .attachments
        .as_ref()
        .is_some_and(|a| !a.is_empty())
    {
        return Ok(String::new());
    }
    Err(anyhow!(
        "webhook body must contain `text`, `message`, `messages`, or `attachments`"
    ))
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
    fn structured_json_path_unchanged() {
        let raw = br#"{"message":"hello","name":"Test"}"#;
        let parsed = parse_webhook_body(raw, "github").unwrap();
        assert!(!parsed.used_raw_body_fallback);
        assert_eq!(parsed.inbound.message.as_deref(), Some("hello"));
    }

    #[test]
    fn github_push_payload_falls_back_to_raw_json() {
        let raw = br#"{"ref":"refs/heads/main","repository":{"full_name":"org/repo"},"pusher":{"name":"alice"}}"#;
        let parsed = parse_webhook_body(raw, "github").unwrap();
        assert!(parsed.used_raw_body_fallback);
        assert_eq!(parsed.inbound.name.as_deref(), Some("github"));
        assert!(parsed.inbound.text.as_ref().unwrap().contains("refs/heads/main"));
        let s = store();
        let conv = "webhook:github:20260628";
        let msgs = build_webhook_dispatch_messages(&s, conv, &parsed.inbound).unwrap();
        assert!(msgs[0].content.starts_with("[github]"));
        assert!(msgs[0].content.contains("org/repo"));
    }

    #[test]
    fn plain_text_body_fallback() {
        let parsed = parse_webhook_body(b"plain webhook ping", "ping").unwrap();
        assert!(parsed.used_raw_body_fallback);
        assert_eq!(parsed.inbound.text.as_deref(), Some("plain webhook ping"));
    }

    #[test]
    fn rejects_oversized_body() {
        let huge = vec![b'x'; MAX_WEBHOOK_BODY_BYTES + 1];
        assert!(matches!(
            parse_webhook_body(&huge, "x"),
            Err(WebhookParseError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn empty_text_with_github_fields_falls_back() {
        let raw = br#"{"text":"","ref":"refs/heads/main"}"#;
        let parsed = parse_webhook_body(raw, "github").unwrap();
        assert!(parsed.used_raw_body_fallback);
    }

    #[test]
    fn text_with_attachments_metadata() {
        let raw = br#"{
            "text":"see file",
            "attachments":[{
                "id":"a1",
                "kind":"document",
                "mimeType":"text/plain",
                "fileName":"note.txt",
                "contentBase64":"aGVsbG8="
            }]
        }"#;
        let parsed = parse_webhook_body(raw, "ci").unwrap();
        assert!(!parsed.used_raw_body_fallback);
        let atts = parsed.inbound.attachments.as_ref().unwrap();
        assert_eq!(atts.len(), 1);
        let s = store();
        let conv = "webhook:ci:20260629";
        let msgs = build_webhook_dispatch_messages(&s, conv, &parsed.inbound).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "see file");
        assert_eq!(msgs[0].attachments.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn attachments_only_without_text() {
        let raw = br#"{
            "attachments":[{
                "id":"a1",
                "kind":"document",
                "mimeType":"text/plain",
                "fileName":"note.txt",
                "contentBase64":"aGVsbG8="
            }]
        }"#;
        let parsed = parse_webhook_body(raw, "ci").unwrap();
        assert!(!parsed.used_raw_body_fallback);
        let s = store();
        let conv = "webhook:ci:20260629";
        let msgs = build_webhook_dispatch_messages(&s, conv, &parsed.inbound).unwrap();
        assert_eq!(msgs[0].content, "");
        assert!(msgs[0].attachments.as_ref().is_some());
    }
}
