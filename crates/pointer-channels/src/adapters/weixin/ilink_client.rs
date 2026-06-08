use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::credentials::{load_sync_cursor, save_sync_cursor};
use crate::http_client::HttpClient;

pub const DEFAULT_BASE_URL: &str = "https://ilinkai.weixin.qq.com";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeixinCredentials {
    pub bot_token: String,
    pub account_id: String,
    pub base_url: String,
    pub user_id: String,
}

#[derive(Clone)]
pub struct ILinkClient {
    http: HttpClient,
    creds: WeixinCredentials,
    channel_account_id: String,
}

impl ILinkClient {
    pub fn new(channel_account_id: String, creds: WeixinCredentials) -> Self {
        Self {
            http: HttpClient::default(),
            creds,
            channel_account_id,
        }
    }

    fn base(&self) -> String {
        let b = self.creds.base_url.trim();
        if b.is_empty() {
            DEFAULT_BASE_URL.into()
        } else {
            b.trim_end_matches('/').to_string()
        }
    }

    fn auth_headers(&self) -> Vec<(String, String)> {
        let uin: u32 = rand::thread_rng().gen();
        let uin_b64 = B64.encode(uin.to_le_bytes());
        vec![
            ("AuthorizationType".into(), "ilink_bot_token".into()),
            ("Authorization".into(), format!("Bearer {}", self.creds.bot_token)),
            ("X-WECHAT-UIN".into(), uin_b64),
            ("Content-Type".into(), "application/json".into()),
        ]
    }

    pub async fn get_updates(&self, timeout_secs: u64) -> Result<Value> {
        let cursor = load_sync_cursor("weixin", &self.channel_account_id)?.unwrap_or_default();
        let url = format!("{}/ilink/bot/getupdates", self.base());
        let body = json!({
            "timeout": timeout_secs,
            "cursor": cursor,
        });
        let auth = self.auth_headers();
        let headers: Vec<(&str, &str)> = auth
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let resp = self.http.post_json(&url, &headers, &body).await?;
        if let Some(next) = resp.get("cursor").and_then(|v| v.as_str()) {
            save_sync_cursor("weixin", &self.channel_account_id, next)?;
        }
        Ok(resp)
    }

    pub async fn send_text(
        &self,
        to_user: &str,
        text: &str,
        context_token: Option<&str>,
    ) -> Result<()> {
        let url = format!("{}/ilink/bot/sendmessage", self.base());
        let mut body = json!({
            "to_user": to_user,
            "items": [{ "type": "text", "text": text }],
        });
        if let Some(token) = context_token {
            body["context_token"] = json!(token);
        }
        let auth = self.auth_headers();
        let headers: Vec<(&str, &str)> = auth
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        self.http.post_json(&url, &headers, &body).await?;
        Ok(())
    }

    pub async fn fetch_qrcode(&self) -> Result<Value> {
        let url = format!("{}/ilink/bot/get_bot_qrcode?bot_type=3", self.base());
        self.http.get_json(&url, &[]).await
    }

    pub async fn poll_qrcode_status(&self, qrcode: &str) -> Result<Value> {
        let url = format!(
            "{}/ilink/bot/get_qrcode_status?qrcode={}",
            self.base(),
            urlencoding::encode(qrcode)
        );
        self.http.get_json(&url, &[]).await
    }
}

pub fn parse_login_info(value: &Value) -> Result<WeixinCredentials> {
    Ok(WeixinCredentials {
        bot_token: value
            .get("botToken")
            .or_else(|| value.get("bot_token"))
            .and_then(|v| v.as_str())
            .context("botToken")?
            .to_string(),
        account_id: value
            .get("accountId")
            .or_else(|| value.get("account_id"))
            .and_then(|v| v.as_str())
            .context("accountId")?
            .to_string(),
        base_url: value
            .get("baseUrl")
            .or_else(|| value.get("base_url"))
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_BASE_URL)
            .to_string(),
        user_id: value
            .get("userId")
            .or_else(|| value.get("user_id"))
            .and_then(|v| v.as_str())
            .context("userId")?
            .to_string(),
    })
}
