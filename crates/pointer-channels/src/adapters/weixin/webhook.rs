use async_trait::async_trait;
use serde_json::Value;

use crate::traits::{ChannelWebhookAdapter, InboundMessage, WebhookContext, WebhookResponse};

/// Weixin uses long-poll monitor, not HTTP webhook inbound.
pub struct WeixinWebhook;

#[async_trait]
impl ChannelWebhookAdapter for WeixinWebhook {
    fn channel_id(&self) -> crate::traits::ChannelId {
        "weixin"
    }

    async fn handle_webhook(&self, _ctx: WebhookContext<'_>) -> anyhow::Result<WebhookResponse> {
        Ok(WebhookResponse {
            status: 404,
            content_type: "text/plain".into(),
            body: b"weixin uses long-poll monitor".to_vec(),
        })
    }

    fn parse_inbound(&self, _event: &Value, _account_id: &str) -> Option<InboundMessage> {
        None
    }
}
