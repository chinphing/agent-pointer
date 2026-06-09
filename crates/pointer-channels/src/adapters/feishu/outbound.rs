use async_trait::async_trait;
use serde_json::json;

use crate::http_client::HttpClient;
use crate::outbound_format::feishu_post_md_content;
use crate::traits::{ChannelOutboundAdapter, OutboundContext};

#[derive(Default)]
pub struct FeishuOutbound {
    http: HttpClient,
}

impl FeishuOutbound {
    async fn tenant_token(&self, app_id: &str, app_secret: &str) -> anyhow::Result<String> {
        let cache_key = format!("feishu:{app_id}");
        if let Some(t) = self.http.get_cached_token(&cache_key) {
            return Ok(t);
        }
        let url = "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
        let body = json!({ "app_id": app_id, "app_secret": app_secret });
        let resp = self.http.post_json(url, &[], &body).await?;
        let token = resp
            .get("tenant_access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing tenant_access_token"))?
            .to_string();
        let expire = resp
            .get("expire")
            .and_then(|v| v.as_u64())
            .unwrap_or(7200);
        self.http.set_cached_token(&cache_key, token.clone(), expire);
        Ok(token)
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
        let token = self
            .tenant_token(&account.app_id, &account.app_secret)
            .await?;
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
        let url = "https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type="
            .to_string()
            + receive_id_type;
        let body = json!({
            "receive_id": receive_id,
            "msg_type": "post",
            "content": feishu_post_md_content(text)?,
        });
        let auth = format!("Bearer {token}");
        let headers = [("Authorization", auth.as_str())];
        self.http.post_json(&url, &headers, &body).await?;
        log::info!("feishu outbound post-md account={}", ctx.account_id);
        Ok(())
    }
}
