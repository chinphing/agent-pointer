use async_trait::async_trait;
use serde_json::json;

use super::auth::{auth_header, tenant_access_token};
use crate::http_client::HttpClient;
use crate::outbound_format::feishu_post_md_content;
use crate::traits::{ChannelOutboundAdapter, OutboundContext, OutboundMedia};

#[derive(Default)]
pub struct FeishuOutbound {
    http: HttpClient,
}

impl FeishuOutbound {
    async fn receive_target(
        &self,
        ctx: &OutboundContext,
    ) -> anyhow::Result<(String, &'static str)> {
        let receive_id = ctx
            .reply_context
            .as_ref()
            .and_then(|r| r.chat_id.clone())
            .or_else(|| ctx.reply_context.as_ref().and_then(|r| r.open_id.clone()))
            .unwrap_or(ctx.recipient_id.clone());
        let receive_id_type = if receive_id.starts_with("oc_") {
            "chat_id"
        } else {
            "open_id"
        };
        Ok((receive_id, receive_id_type))
    }

    async fn upload_image(
        &self,
        token: &str,
        media: &OutboundMedia,
    ) -> anyhow::Result<String> {
        let url = "https://open.feishu.cn/open-apis/im/v1/images";
        let auth = auth_header(token);
        let headers = [("Authorization", auth.as_str())];
        let parts = vec![
            ("image_type", b"message".to_vec(), None),
            (
                "image",
                media.bytes.clone(),
                Some(media.file_name.clone()),
            ),
        ];
        let resp = self.http.post_multipart(url, &headers, parts).await?;
        let key = resp
            .get("data")
            .and_then(|d| d.get("image_key"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("feishu image upload missing image_key"))?;
        Ok(key.to_string())
    }

    async fn upload_file(
        &self,
        token: &str,
        media: &OutboundMedia,
    ) -> anyhow::Result<String> {
        let url = "https://open.feishu.cn/open-apis/im/v1/files";
        let auth = auth_header(token);
        let headers = [("Authorization", auth.as_str())];
        let file_type = if media.mime_type.starts_with("audio/") {
            "opus"
        } else if media.mime_type.starts_with("video/") {
            "mp4"
        } else {
            "stream"
        };
        let parts = vec![
            ("file_type", file_type.as_bytes().to_vec(), None),
            (
                "file",
                media.bytes.clone(),
                Some(media.file_name.clone()),
            ),
        ];
        let resp = self.http.post_multipart(url, &headers, parts).await?;
        let key = resp
            .get("data")
            .and_then(|d| d.get("file_key"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("feishu file upload missing file_key"))?;
        Ok(key.to_string())
    }

    async fn send_message(
        &self,
        token: &str,
        receive_id: &str,
        receive_id_type: &str,
        msg_type: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        let url = format!(
            "https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type={receive_id_type}"
        );
        let body = json!({
            "receive_id": receive_id,
            "msg_type": msg_type,
            "content": content,
        });
        let auth = auth_header(token);
        let headers = [("Authorization", auth.as_str())];
        self.http.post_json(&url, &headers, &body).await?;
        Ok(())
    }
}

#[async_trait]
impl ChannelOutboundAdapter for FeishuOutbound {
    fn channel_id(&self) -> crate::traits::ChannelId {
        "feishu"
    }

    async fn send_text(&self, ctx: OutboundContext, text: &str) -> anyhow::Result<()> {
        let cfg = crate::config::load_channels_config()?;
        let account = cfg
            .account("feishu", &ctx.account_id)
            .ok_or_else(|| anyhow::anyhow!("feishu account missing"))?;
        let token = tenant_access_token(&self.http, &account.app_id, &account.app_secret).await?;
        let (receive_id, receive_id_type) = self.receive_target(&ctx).await?;
        let content = feishu_post_md_content(text)?;
        self.send_message(&token, &receive_id, receive_id_type, "post", &content)
            .await?;
        log::info!("feishu outbound post-md account={}", ctx.account_id);
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
            .account("feishu", &ctx.account_id)
            .ok_or_else(|| anyhow::anyhow!("feishu account missing"))?;
        let token = tenant_access_token(&self.http, &account.app_id, &account.app_secret).await?;
        let (receive_id, receive_id_type) = self.receive_target(&ctx).await?;

        if let Some(text) = caption.filter(|s| !s.trim().is_empty()) {
            let content = feishu_post_md_content(text)?;
            self.send_message(&token, &receive_id, receive_id_type, "post", &content)
                .await?;
        }

        if media.is_image() {
            let image_key = self.upload_image(&token, &media).await?;
            let content = serde_json::to_string(&json!({ "image_key": image_key }))?;
            self.send_message(&token, &receive_id, receive_id_type, "image", &content)
                .await?;
        } else {
            let file_key = self.upload_file(&token, &media).await?;
            let content = serde_json::to_string(&json!({ "file_key": file_key }))?;
            self.send_message(&token, &receive_id, receive_id_type, "file", &content)
                .await?;
        }
        log::info!(
            "feishu outbound media account={} file={}",
            ctx.account_id,
            media.file_name
        );
        Ok(())
    }
}
