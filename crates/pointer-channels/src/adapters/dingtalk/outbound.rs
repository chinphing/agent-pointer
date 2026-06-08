use async_trait::async_trait;
use serde_json::json;

use crate::http_client::HttpClient;
use crate::traits::{ChannelOutboundAdapter, OutboundContext};

#[derive(Default)]
pub struct DingTalkOutbound {
    http: HttpClient,
}

#[async_trait]
impl ChannelOutboundAdapter for DingTalkOutbound {
    fn channel_id(&self) -> crate::traits::ChannelId {
        "dingtalk"
    }

    async fn send_text(&self, ctx: OutboundContext, text: &str) -> anyhow::Result<()> {
        if let Some(url) = ctx
            .reply_context
            .as_ref()
            .and_then(|r| r.session_webhook.clone())
        {
            let body = json!({
                "msgtype": "text",
                "text": { "content": text }
            });
            self.http.post_json(&url, &[], &body).await?;
            return Ok(());
        }
        return Err(anyhow::anyhow!(
            "dingtalk outbound missing sessionWebhook account={}",
            ctx.account_id
        ));
    }
}
