use async_trait::async_trait;
use serde_json::json;

use super::auth::access_token;
use crate::http_client::HttpClient;
use crate::outbound_format::dingtalk_markdown_title;
use crate::traits::{ChannelOutboundAdapter, OutboundContext, OutboundMedia};

#[derive(Default)]
pub struct DingTalkOutbound {
    http: HttpClient,
}

impl DingTalkOutbound {
    async fn session_webhook(&self, ctx: &OutboundContext) -> anyhow::Result<String> {
        ctx.reply_context
            .as_ref()
            .and_then(|r| r.session_webhook.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "dingtalk outbound missing sessionWebhook account={}",
                    ctx.account_id
                )
            })
    }

    async fn upload_media(
        &self,
        account: &crate::config::ChannelAccountConfig,
        media: &OutboundMedia,
    ) -> anyhow::Result<String> {
        let token = access_token(&self.http, &account.client_id, &account.client_secret).await?;
        let media_type = if media.is_image() { "image" } else { "file" };
        let url = format!("https://oapi.dingtalk.com/media/upload?access_token={token}");
        let parts = vec![
            ("type", media_type.as_bytes().to_vec(), None),
            (
                "media",
                media.bytes.clone(),
                Some(media.file_name.clone()),
            ),
        ];
        let resp = self.http.post_multipart(&url, &[], parts).await?;
        let media_id = resp
            .get("media_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("dingtalk upload missing media_id"))?;
        Ok(media_id.to_string())
    }
}

#[async_trait]
impl ChannelOutboundAdapter for DingTalkOutbound {
    fn channel_id(&self) -> crate::traits::ChannelId {
        "dingtalk"
    }

    async fn send_text(&self, ctx: OutboundContext, text: &str) -> anyhow::Result<()> {
        let url = self.session_webhook(&ctx).await?;
        let body = json!({
            "msgtype": "markdown",
            "markdown": {
                "title": dingtalk_markdown_title(text),
                "text": text
            }
        });
        self.http.post_json(&url, &[], &body).await?;
        log::info!("dingtalk outbound markdown account={}", ctx.account_id);
        Ok(())
    }

    async fn send_media(
        &self,
        ctx: OutboundContext,
        caption: Option<&str>,
        media: OutboundMedia,
    ) -> anyhow::Result<()> {
        let cfg = crate::config::load_channels_config()?;
        let account = cfg
            .account("dingtalk", &ctx.account_id)
            .ok_or_else(|| anyhow::anyhow!("dingtalk account missing"))?;
        let url = self.session_webhook(&ctx).await?;

        if let Some(text) = caption.filter(|s| !s.trim().is_empty()) {
            let body = json!({
                "msgtype": "markdown",
                "markdown": {
                    "title": dingtalk_markdown_title(text),
                    "text": text
                }
            });
            self.http.post_json(&url, &[], &body).await?;
        }

        let media_id = self.upload_media(account, &media).await?;
        let body = if media.is_image() {
            json!({
                "msgtype": "image",
                "image": { "media_id": media_id }
            })
        } else {
            json!({
                "msgtype": "file",
                "file": { "media_id": media_id }
            })
        };
        self.http.post_json(&url, &[], &body).await?;
        log::info!(
            "dingtalk outbound media account={} file={}",
            ctx.account_id,
            media.file_name
        );
        Ok(())
    }
}
