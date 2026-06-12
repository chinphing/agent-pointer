use async_trait::async_trait;
use serde_json::{json, Value};

use super::auth::access_token;
use super::oapi::{
    check_oapi_errcode, check_openapi_response, is_group_conversation_key, prepare_oapi_file_upload,
    sample_file_type,
};
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

    async fn post_session_webhook(&self, url: &str, body: &Value, step: &str) -> anyhow::Result<()> {
        let resp = self.http.post_json(url, &[], body).await?;
        check_oapi_errcode(&resp, step)?;
        Ok(())
    }

    async fn upload_media(
        &self,
        account: &crate::config::ChannelAccountConfig,
        media: &OutboundMedia,
    ) -> anyhow::Result<(String, String)> {
        let token = access_token(&self.http, &account.client_id, &account.client_secret).await?;
        let prepared = prepare_oapi_file_upload(media)?;
        let media_type = if media.is_image() { "image" } else { "file" };
        let url = format!(
            "https://oapi.dingtalk.com/media/upload?access_token={token}&type={media_type}"
        );
        let parts = vec![(
            "media",
            prepared.bytes,
            Some(prepared.file_name.clone()),
        )];
        let resp = self.http.post_multipart(&url, &[], parts).await?;
        check_oapi_errcode(&resp, "media/upload")?;
        let media_id = resp
            .get("media_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("dingtalk upload missing media_id response={resp}"))?
            .to_string();
        log::info!(
            "dingtalk oapi media/upload ok type={media_type} file={} zipped={} media_id={media_id}",
            prepared.file_name,
            prepared.zipped
        );
        Ok((media_id, prepared.file_name))
    }

    /// Files cannot be sent via sessionWebhook (`401102 file->mediaId`). Use robot OpenAPI.
    async fn send_proactive_file(
        &self,
        ctx: &OutboundContext,
        account: &crate::config::ChannelAccountConfig,
        media_id: &str,
        upload_file_name: &str,
    ) -> anyhow::Result<()> {
        let token = access_token(&self.http, &account.client_id, &account.client_secret).await?;
        let robot_code = account.client_id.trim();
        if robot_code.is_empty() {
            anyhow::bail!("dingtalk account missing clientId (robotCode)");
        }
        let msg_param = json!({
            "mediaId": media_id,
            "fileName": upload_file_name,
            "fileType": sample_file_type(upload_file_name),
        });
        let is_group = is_group_conversation_key(&ctx.conversation_key);
        let (url, body) = if is_group {
            let open_conversation_id = ctx
                .reply_context
                .as_ref()
                .and_then(|r| r.chat_id.as_deref())
                .ok_or_else(|| anyhow::anyhow!("dingtalk proactive file missing openConversationId"))?;
            (
                "https://api.dingtalk.com/v1.0/robot/groupMessages/send",
                json!({
                    "robotCode": robot_code,
                    "openConversationId": open_conversation_id,
                    "msgKey": "sampleFile",
                    "msgParam": msg_param.to_string(),
                }),
            )
        } else {
            let user_id = ctx.recipient_id.trim();
            if user_id.is_empty() {
                anyhow::bail!("dingtalk proactive file missing userIds");
            }
            (
                "https://api.dingtalk.com/v1.0/robot/oToMessages/batchSend",
                json!({
                    "robotCode": robot_code,
                    "userIds": [user_id],
                    "msgKey": "sampleFile",
                    "msgParam": msg_param.to_string(),
                }),
            )
        };
        let headers = [("x-acs-dingtalk-access-token", token.as_str())];
        let resp = self.http.post_json(url, &headers, &body).await?;
        check_openapi_response(&resp, "robot proactive file")?;
        log::info!(
            "dingtalk proactive file ok account={} file={} group={is_group}",
            ctx.account_id,
            upload_file_name
        );
        Ok(())
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
        self.post_session_webhook(&url, &body, "sessionWebhook markdown")
            .await?;
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

        if let Some(text) = caption.filter(|s| !s.trim().is_empty()) {
            let url = self.session_webhook(&ctx).await?;
            let body = json!({
                "msgtype": "markdown",
                "markdown": {
                    "title": dingtalk_markdown_title(text),
                    "text": text
                }
            });
            self.post_session_webhook(&url, &body, "sessionWebhook markdown caption")
                .await?;
        }

        let (media_id, upload_name) = self.upload_media(account, &media).await?;
        if media.is_image() {
            let url = self.session_webhook(&ctx).await?;
            let body = json!({
                "msgtype": "image",
                "image": { "media_id": media_id }
            });
            self.post_session_webhook(&url, &body, "sessionWebhook image")
                .await?;
        } else {
            self.send_proactive_file(&ctx, account, &media_id, &upload_name)
                .await?;
        }
        log::info!(
            "dingtalk outbound media account={} file={}",
            ctx.account_id,
            media.file_name
        );
        Ok(())
    }
}
