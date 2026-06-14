use async_trait::async_trait;
use serde_json::json;

use super::auth::{access_token, check_wecom_api_response, invalidate_access_token};
use super::ws_state::{get_session, is_connected};
use super::ws_upload;
use crate::http_client::HttpClient;
use crate::traits::{ChannelOutboundAdapter, OutboundContext, OutboundMedia};

#[derive(Default)]
pub struct WeComOutbound {
    http: HttpClient,
}

impl WeComOutbound {
    fn wecom_media_type(media: &OutboundMedia) -> &'static str {
        if media.is_image() {
            "image"
        } else if media.mime_type.starts_with("video/") {
            "video"
        } else if media.mime_type.starts_with("audio/") {
            "voice"
        } else {
            "file"
        }
    }

    async fn upload_via_agent_http(
        &self,
        account: &crate::config::ChannelAccountConfig,
        media: &OutboundMedia,
    ) -> anyhow::Result<String> {
        let corp_id = account.corp_id.as_str();
        let secret = account.secret.as_str();
        let media_type = Self::wecom_media_type(media);
        let token = access_token(&self.http, corp_id, secret).await?;
        match self
            .upload_media_with_token(&token, media_type, media)
            .await
        {
            Ok(id) => Ok(id),
            Err(e) if crate::token_cache::is_wecom_invalid_token_error(&e) => {
                log::warn!("wecom token rejected on media/upload; refreshing and retrying once");
                invalidate_access_token(&self.http, corp_id);
                let token = access_token(&self.http, corp_id, secret).await?;
                self.upload_media_with_token(&token, media_type, media)
                    .await
            }
            Err(e) => Err(e),
        }
    }

    async fn upload_media_with_token(
        &self,
        token: &str,
        media_type: &str,
        media: &OutboundMedia,
    ) -> anyhow::Result<String> {
        let url = format!(
            "https://qyapi.weixin.qq.com/cgi-bin/media/upload?access_token={token}&type={media_type}"
        );
        let parts = vec![(
            "media",
            media.bytes.clone(),
            Some(media.file_name.clone()),
        )];
        let resp = self.http.post_multipart(&url, &[], parts).await?;
        check_wecom_api_response(&resp, "media/upload")?;
        resp.get("media_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("wecom agent upload missing media_id"))
            .map(|s| s.to_string())
    }

    async fn send_via_agent_http(
        &self,
        account: &crate::config::ChannelAccountConfig,
        ctx: &OutboundContext,
        text: &str,
    ) -> anyhow::Result<()> {
        if account.corp_id.trim().is_empty()
            || account.agent_id.trim().is_empty()
            || account.secret.trim().is_empty()
        {
            return Err(anyhow::anyhow!(
                "wecom agent HTTP fallback missing corpId/agentId/secret"
            ));
        }
        let corp_id = account.corp_id.as_str();
        let secret = account.secret.as_str();
        let token = access_token(&self.http, corp_id, secret).await?;
        match self
            .send_text_with_token(account, ctx, text, &token)
            .await
        {
            Ok(()) => Ok(()),
            Err(e) if crate::token_cache::is_wecom_invalid_token_error(&e) => {
                log::warn!("wecom token rejected on message/send; refreshing and retrying once");
                invalidate_access_token(&self.http, corp_id);
                let token = access_token(&self.http, corp_id, secret).await?;
                self.send_text_with_token(account, ctx, text, &token).await
            }
            Err(e) => Err(e),
        }
    }

    async fn send_text_with_token(
        &self,
        account: &crate::config::ChannelAccountConfig,
        ctx: &OutboundContext,
        text: &str,
        token: &str,
    ) -> anyhow::Result<()> {
        let url = format!("https://qyapi.weixin.qq.com/cgi-bin/message/send?access_token={token}");
        let body = json!({
            "touser": ctx.recipient_id,
            "msgtype": "text",
            "agentid": account.agent_id.parse::<i64>().unwrap_or(0),
            "text": { "content": text }
        });
        let resp = self.http.post_json(&url, &[], &body).await?;
        check_wecom_api_response(&resp, "message/send")
    }

    async fn send_media_via_agent_http(
        &self,
        account: &crate::config::ChannelAccountConfig,
        ctx: &OutboundContext,
        media_type: &str,
        media_id: &str,
    ) -> anyhow::Result<()> {
        let corp_id = account.corp_id.as_str();
        let secret = account.secret.as_str();
        let token = access_token(&self.http, corp_id, secret).await?;
        match self
            .send_media_with_token(account, ctx, media_type, media_id, &token)
            .await
        {
            Ok(()) => Ok(()),
            Err(e) if crate::token_cache::is_wecom_invalid_token_error(&e) => {
                log::warn!("wecom token rejected on media/send; refreshing and retrying once");
                invalidate_access_token(&self.http, corp_id);
                let token = access_token(&self.http, corp_id, secret).await?;
                self.send_media_with_token(account, ctx, media_type, media_id, &token)
                    .await
            }
            Err(e) => Err(e),
        }
    }

    async fn send_media_with_token(
        &self,
        account: &crate::config::ChannelAccountConfig,
        ctx: &OutboundContext,
        media_type: &str,
        media_id: &str,
        token: &str,
    ) -> anyhow::Result<()> {
        let url = format!("https://qyapi.weixin.qq.com/cgi-bin/message/send?access_token={token}");
        let mut body = json!({
            "touser": ctx.recipient_id,
            "msgtype": media_type,
            "agentid": account.agent_id.parse::<i64>().unwrap_or(0),
        });
        if let Some(obj) = body.as_object_mut() {
            obj.insert(
                media_type.to_string(),
                json!({ "media_id": media_id }),
            );
        }
        let resp = self.http.post_json(&url, &[], &body).await?;
        check_wecom_api_response(&resp, "message/send media")
    }
}

#[async_trait]
impl ChannelOutboundAdapter for WeComOutbound {
    fn channel_id(&self) -> crate::traits::ChannelId {
        "wecom"
    }

    async fn send_text(&self, ctx: OutboundContext, text: &str) -> anyhow::Result<()> {
        let cfg = crate::config::load_channels_config()?;
        let account = cfg
            .account("wecom", &ctx.account_id)
            .ok_or_else(|| anyhow::anyhow!("wecom account missing"))?;

        if account.connection_mode == "websocket" && is_connected(&ctx.account_id) {
            if let Some(session) = get_session(&ctx.account_id) {
                if let Some(reply) = &ctx.reply_context {
                    if let Some(req_id) = &reply.wecom_req_id {
                        let stream_id = uuid::Uuid::new_v4().to_string();
                        session.send_stream_reply(req_id, &stream_id, text, true)?;
                        log::info!("wecom ws outbound stream reply account={}", ctx.account_id);
                        return Ok(());
                    }
                    if let Some(chat_id) = &reply.chat_id {
                        session.send_markdown(chat_id, text)?;
                        log::info!("wecom ws outbound proactive account={}", ctx.account_id);
                        return Ok(());
                    }
                }
                let chat_id = ctx
                    .reply_context
                    .as_ref()
                    .and_then(|r| r.chat_id.clone())
                    .unwrap_or_else(|| ctx.recipient_id.clone());
                session.send_markdown(&chat_id, text)?;
                log::info!("wecom ws outbound markdown account={}", ctx.account_id);
                return Ok(());
            }
        }

        self.send_via_agent_http(account, &ctx, text).await
    }

    async fn send_media(
        &self,
        ctx: OutboundContext,
        caption: Option<&str>,
        media: OutboundMedia,
    ) -> anyhow::Result<()> {
        let cfg = crate::config::load_channels_config()?;
        let account = cfg
            .account("wecom", &ctx.account_id)
            .ok_or_else(|| anyhow::anyhow!("wecom account missing"))?;
        let media_type = Self::wecom_media_type(&media);

        if account.connection_mode == "websocket" && is_connected(&ctx.account_id) {
            if let Some(session) = get_session(&ctx.account_id) {
                if let Some(text) = caption.filter(|s| !s.trim().is_empty()) {
                    if let Some(reply) = &ctx.reply_context {
                        if let Some(req_id) = &reply.wecom_req_id {
                            let stream_id = uuid::Uuid::new_v4().to_string();
                            session.send_stream_reply(req_id, &stream_id, text, true)?;
                        } else if let Some(chat_id) = &reply.chat_id {
                            session.send_markdown(chat_id, text)?;
                        } else {
                            session.send_markdown(&ctx.recipient_id, text)?;
                        }
                    } else {
                        session.send_markdown(&ctx.recipient_id, text)?;
                    }
                }

                let media_id = ws_upload::upload_media(
                    &session,
                    &media.bytes,
                    media_type,
                    &media.file_name,
                )
                .await?;

                if let Some(req_id) = ctx
                    .reply_context
                    .as_ref()
                    .and_then(|r| r.wecom_req_id.clone())
                {
                    session.respond_media(&req_id, media_type, &media_id)?;
                } else {
                    let chat_id = ctx
                        .reply_context
                        .as_ref()
                        .and_then(|r| r.chat_id.clone())
                        .unwrap_or_else(|| ctx.recipient_id.clone());
                    session.send_media(&chat_id, media_type, &media_id)?;
                }
                log::info!(
                    "wecom ws outbound media account={} file={}",
                    ctx.account_id,
                    media.file_name
                );
                return Ok(());
            }
        }

        if let Some(text) = caption.filter(|s| !s.trim().is_empty()) {
            self.send_via_agent_http(account, &ctx, text).await?;
        }
        let media_id = self.upload_via_agent_http(account, &media).await?;
        self.send_media_via_agent_http(account, &ctx, media_type, &media_id)
            .await?;
        log::info!(
            "wecom agent outbound media account={} file={}",
            ctx.account_id,
            media.file_name
        );
        Ok(())
    }
}
