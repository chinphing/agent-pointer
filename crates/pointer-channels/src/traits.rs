use async_trait::async_trait;
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use crate::config::ChannelAccountConfig;

pub type ChannelId = &'static str;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundMessage {
    pub channel: String,
    pub account_id: String,
    pub message_id: String,
    pub conversation_key: String,
    pub sender_id: String,
    pub sender_name: Option<String>,
    pub text: String,
    pub is_group: bool,
    pub mentioned_bot: bool,
    #[serde(default)]
    pub reply_context: Option<InboundReplyContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundReplyContext {
    pub session_webhook: Option<String>,
    pub chat_id: Option<String>,
    pub open_id: Option<String>,
    pub context_token: Option<String>,
    /// WeCom WebSocket passive reply req_id (from callback frame headers).
    #[serde(default, rename = "wecomReqId")]
    pub wecom_req_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WebhookResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

pub struct WebhookContext<'a> {
    pub channel: ChannelId,
    pub account_id: &'a str,
    pub account: &'a ChannelAccountConfig,
    pub headers: &'a HeaderMap,
    pub raw_body: &'a [u8],
    pub method: &'a str,
    pub query: &'a str,
}

pub struct OutboundContext {
    pub channel: String,
    pub account_id: String,
    pub conversation_key: String,
    pub recipient_id: String,
    pub reply_context: Option<InboundReplyContext>,
}

#[async_trait]
pub trait ChannelWebhookAdapter: Send + Sync {
    fn channel_id(&self) -> ChannelId;

    async fn handle_webhook(&self, ctx: WebhookContext<'_>) -> anyhow::Result<WebhookResponse>;

    fn parse_inbound(&self, event: &Value, account_id: &str) -> Option<InboundMessage>;
}

#[async_trait]
pub trait ChannelOutboundAdapter: Send + Sync {
    fn channel_id(&self) -> ChannelId;

    async fn send_text(&self, ctx: OutboundContext, text: &str) -> anyhow::Result<()>;
}

pub struct ChannelPlugin {
    pub webhook: Arc<dyn ChannelWebhookAdapter>,
    pub outbound: Arc<dyn ChannelOutboundAdapter>,
}

impl ChannelPlugin {
    pub fn new(
        webhook: Arc<dyn ChannelWebhookAdapter>,
        outbound: Arc<dyn ChannelOutboundAdapter>,
    ) -> Self {
        Self { webhook, outbound }
    }

    pub fn channel_id(&self) -> ChannelId {
        self.webhook.channel_id()
    }
}
