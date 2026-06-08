use async_trait::async_trait;

use super::ilink_client::{ILinkClient, WeixinCredentials};
use crate::credentials::load_encrypted_json;
use crate::traits::{ChannelOutboundAdapter, OutboundContext};

#[derive(Default)]
pub struct WeixinOutbound;

#[async_trait]
impl ChannelOutboundAdapter for WeixinOutbound {
    fn channel_id(&self) -> crate::traits::ChannelId {
        "weixin"
    }

    async fn send_text(&self, ctx: OutboundContext, text: &str) -> anyhow::Result<()> {
        let creds: WeixinCredentials = load_encrypted_json("weixin", &ctx.account_id)?
            .ok_or_else(|| anyhow::anyhow!("weixin credentials missing"))?;
        let client = ILinkClient::new(ctx.account_id.clone(), creds);
        let context_token = ctx
            .reply_context
            .as_ref()
            .and_then(|r| r.context_token.as_deref());
        client
            .send_text(&ctx.recipient_id, text, context_token)
            .await
    }
}
