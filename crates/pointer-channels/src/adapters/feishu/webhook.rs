use async_trait::async_trait;
use serde_json::Value;

use crate::crypto::{constant_time_eq, feishu_webhook_signature};
use crate::session::build_conversation_key;
use crate::traits::{
    ChannelWebhookAdapter, InboundMessage, InboundReplyContext, WebhookContext, WebhookResponse,
};

pub struct FeishuWebhook;

#[async_trait]
impl ChannelWebhookAdapter for FeishuWebhook {
    fn channel_id(&self) -> crate::traits::ChannelId {
        "feishu"
    }

    async fn handle_webhook(&self, ctx: WebhookContext<'_>) -> anyhow::Result<WebhookResponse> {
        let raw = std::str::from_utf8(ctx.raw_body).unwrap_or("");
        let encrypt_key = ctx.account.encrypt_key.trim();
        if !encrypt_key.is_empty() {
            let timestamp = header_str(ctx.headers, "x-lark-request-timestamp");
            let nonce = header_str(ctx.headers, "x-lark-request-nonce");
            let signature = header_str(ctx.headers, "x-lark-signature");
            if timestamp.is_empty() || nonce.is_empty() || signature.is_empty() {
                return Ok(json_response(401, r#"{"error":"missing signature headers"}"#));
            }
            let computed = feishu_webhook_signature(&timestamp, &nonce, encrypt_key, raw);
            if !constant_time_eq(&computed, &signature) {
                log::warn!("feishu webhook invalid signature account={}", ctx.account_id);
                return Ok(text_response(401, "Invalid signature"));
            }
        }

        let payload: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
        if payload.get("type").and_then(|v| v.as_str()) == Some("url_verification") {
            let challenge = payload
                .get("challenge")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            return Ok(json_response(
                200,
                &format!(r#"{{"challenge":"{challenge}"}}"#),
            ));
        }

        Ok(text_response(200, "ok"))
    }

    fn parse_inbound(&self, event: &Value, account_id: &str) -> Option<InboundMessage> {
        let header = event.get("header")?;
        let event_type = header.get("event_type")?.as_str()?;
        if event_type != "im.message.receive_v1" {
            return None;
        }
        let ev = event.get("event")?;
        let message = ev.get("message")?;
        let sender = ev.get("sender")?;
        let message_id = message.get("message_id")?.as_str()?;
        let chat_id = message.get("chat_id")?.as_str()?;
        let chat_type = message.get("chat_type")?.as_str().unwrap_or("p2p");
        let is_group = chat_type == "group" || chat_type == "topic_group";
        let content_raw = message.get("content")?.as_str().unwrap_or("{}");
        let content: Value = serde_json::from_str(content_raw).ok()?;
        let text = content
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if text.trim().is_empty() {
            return None;
        }
        let sender_id = sender
            .get("sender_id")
            .and_then(|s| s.get("open_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let mentions = message.get("mentions").and_then(|v| v.as_array());
        let mentioned_bot = mentions.map(|m| !m.is_empty()).unwrap_or(false);

        Some(InboundMessage {
            channel: "feishu".into(),
            account_id: account_id.into(),
            message_id: message_id.into(),
            conversation_key: build_conversation_key("feishu", chat_id, is_group),
            sender_id,
            sender_name: None,
            text,
            is_group,
            mentioned_bot,
            reply_context: Some(InboundReplyContext {
                session_webhook: None,
                chat_id: Some(chat_id.into()),
                open_id: None,
                context_token: None,
                wecom_req_id: None,
            }),
        })
    }
}

fn header_str(headers: &axum::http::HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

fn text_response(status: u16, body: &str) -> WebhookResponse {
    WebhookResponse {
        status,
        content_type: "text/plain; charset=utf-8".into(),
        body: body.as_bytes().to_vec(),
    }
}

fn json_response(status: u16, body: &str) -> WebhookResponse {
    WebhookResponse {
        status,
        content_type: "application/json; charset=utf-8".into(),
        body: body.as_bytes().to_vec(),
    }
}
