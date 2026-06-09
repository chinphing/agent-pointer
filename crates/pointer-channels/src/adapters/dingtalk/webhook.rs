use async_trait::async_trait;
use serde_json::Value;

use crate::traits::{
    ChannelWebhookAdapter, WebhookContext, WebhookResponse,
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

    fn parse_inbound(&self, event: &Value, account_id: &str) -> Option<crate::traits::InboundMessage> {
        super::parse::parse_inbound(event, account_id)
    }
}
