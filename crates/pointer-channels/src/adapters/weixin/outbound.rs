use async_trait::async_trait;
use serde_json::Value;

use super::cdn_upload::{file_item_json, image_item_json, upload_weixin_media};
use super::ilink_client::{ILinkClient, WeixinCredentials};
use crate::credentials::load_encrypted_json;
use crate::traits::{ChannelOutboundAdapter, OutboundContext, OutboundMedia};

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

    async fn send_media(
        &self,
        ctx: OutboundContext,
        caption: Option<&str>,
        media: OutboundMedia,
    ) -> anyhow::Result<()> {
        let creds: WeixinCredentials = load_encrypted_json("weixin", &ctx.account_id)?
            .ok_or_else(|| anyhow::anyhow!("weixin credentials missing"))?;
        let client = ILinkClient::new(ctx.account_id.clone(), creds);
        let context_token = ctx
            .reply_context
            .as_ref()
            .and_then(|r| r.context_token.as_deref())
            .ok_or_else(|| anyhow::anyhow!("weixin send media requires context_token"))?;

        let uploaded = upload_weixin_media(
            &client,
            &ctx.recipient_id,
            &media.file_name,
            &media.mime_type,
            &media.bytes,
        )
        .await?;

        let mut items: Vec<Value> = Vec::new();
        if let Some(text) = caption.filter(|s| !s.trim().is_empty()) {
            items.push(serde_json::json!({
                "type": 1,
                "text_item": { "text": text }
            }));
        }
        if media.is_image() {
            items.push(image_item_json(&uploaded));
        } else {
            items.push(file_item_json(&uploaded, &media.file_name));
        }

        client
            .send_message_items(&ctx.recipient_id, context_token, &mut items)
            .await?;
        log::info!(
            "weixin outbound media account={} file={}",
            ctx.account_id,
            media.file_name
        );
        Ok(())
    }
}
