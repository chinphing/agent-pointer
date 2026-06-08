use async_trait::async_trait;
use serde_json::json;

use super::ws_state::{get_session, is_connected};
use crate::http_client::HttpClient;
use crate::traits::{ChannelOutboundAdapter, OutboundContext};

#[derive(Default)]
pub struct WeComOutbound {
    http: HttpClient,
}

impl WeComOutbound {
    async fn access_token(&self, corp_id: &str, secret: &str) -> anyhow::Result<String> {
        let key = format!("wecom:{corp_id}");
        if let Some(t) = self.http.get_cached_token(&key) {
            return Ok(t);
        }
        let url = format!(
            "https://qyapi.weixin.qq.com/cgi-bin/gettoken?corpid={corp_id}&corpsecret={secret}"
        );
        let resp = self.http.get_json(&url, &[]).await?;
        let token = resp
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing access_token"))?
            .to_string();
        self.http.set_cached_token(&key, token.clone(), 7200);
        Ok(token)
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
        let token = self
            .access_token(&account.corp_id, &account.secret)
            .await?;
        let url = format!("https://qyapi.weixin.qq.com/cgi-bin/message/send?access_token={token}");
        let body = json!({
            "touser": ctx.recipient_id,
            "msgtype": "text",
            "agentid": account.agent_id.parse::<i64>().unwrap_or(0),
            "text": { "content": text }
        });
        self.http.post_json(&url, &[], &body).await?;
        Ok(())
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
}
