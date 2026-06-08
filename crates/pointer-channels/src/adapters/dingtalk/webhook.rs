use async_trait::async_trait;
use serde_json::Value;

use crate::session::build_conversation_key;
use crate::traits::{
    ChannelWebhookAdapter, InboundMessage, InboundReplyContext, WebhookContext, WebhookResponse,
};

pub struct DingTalkWebhook;

#[async_trait]
impl ChannelWebhookAdapter for DingTalkWebhook {
    fn channel_id(&self) -> crate::traits::ChannelId {
        "dingtalk"
    }

    async fn handle_webhook(&self, ctx: WebhookContext<'_>) -> anyhow::Result<WebhookResponse> {
        let _ = ctx;
        Ok(WebhookResponse {
            status: 200,
            content_type: "application/json".into(),
            body: br#"{"errcode":0,"errmsg":"ok"}"#.to_vec(),
        })
    }

    fn parse_inbound(&self, event: &Value, account_id: &str) -> Option<InboundMessage> {
        let text = event
            .get("text")
            .and_then(|t| t.get("content"))
            .and_then(|v| v.as_str())
            .or_else(|| event.get("content").and_then(|v| v.as_str()))?;
        let message_id = event
            .get("msgId")
            .and_then(|v| v.as_str())
            .or_else(|| event.get("msgid").and_then(|v| v.as_str()))
            .unwrap_or("unknown")
            .to_string();
        let sender_id = event
            .get("senderStaffId")
            .or_else(|| event.get("senderId"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let conversation_id = event
            .get("conversationId")
            .and_then(|v| v.as_str())
            .unwrap_or(&sender_id)
            .to_string();
        let conversation_type = event
            .get("conversationType")
            .and_then(|v| v.as_str())
            .unwrap_or("1");
        let is_group = conversation_type == "2";
        let session_webhook = event
            .get("sessionWebhook")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let mentioned = event
            .get("isInAtList")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        Some(InboundMessage {
            channel: "dingtalk".into(),
            account_id: account_id.into(),
            message_id,
            conversation_key: build_conversation_key("dingtalk", &conversation_id, is_group),
            sender_id,
            sender_name: event
                .get("senderNick")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            text: text.to_string(),
            is_group,
            mentioned_bot: mentioned,
            reply_context: Some(InboundReplyContext {
                session_webhook,
                chat_id: Some(conversation_id),
                open_id: None,
                context_token: None,
                wecom_req_id: None,
            }),
        })
    }
}
