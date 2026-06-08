use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::credentials::{load_sync_cursor, save_sync_cursor};
use crate::http_client::HttpClient;

pub const DEFAULT_BASE_URL: &str = "https://ilinkai.weixin.qq.com";
const CHANNEL_VERSION: &str = "1.0.2";

fn wechat_uin_header() -> String {
    let uin: u32 = rand::thread_rng().gen();
    B64.encode(uin.to_string().as_bytes())
}

fn base_info() -> Value {
    json!({ "channel_version": CHANNEL_VERSION })
}

fn check_ilink_ret(resp: &Value, op: &str) -> Result<()> {
    if let Some(ret) = resp.get("ret").and_then(|v| v.as_i64()) {
        if ret != 0 {
            let errmsg = resp
                .get("errmsg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            return Err(anyhow::anyhow!("iLink {op} ret={ret} errmsg={errmsg}"));
        }
    }
    Ok(())
}

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
        vec![
            ("AuthorizationType".into(), "ilink_bot_token".into()),
            ("Authorization".into(), format!("Bearer {}", self.creds.bot_token)),
            ("X-WECHAT-UIN".into(), wechat_uin_header()),
            ("Content-Type".into(), "application/json".into()),
        ]
    }

    pub async fn get_updates(&self, _timeout_secs: u64) -> Result<Value> {
        let buf = load_sync_cursor("weixin", &self.channel_account_id)?.unwrap_or_default();
        let url = format!("{}/ilink/bot/getupdates", self.base());
        let body = json!({
            "get_updates_buf": buf,
            "base_info": base_info(),
        });
        let auth = self.auth_headers();
        let headers: Vec<(&str, &str)> = auth
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let resp = self.http.post_json(&url, &headers, &body).await?;
        check_ilink_ret(&resp, "getupdates")?;
        if let Some(next) = resp.get("get_updates_buf").and_then(|v| v.as_str()) {
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
        let token = context_token.context("weixin sendmessage requires context_token")?;
        let url = format!("{}/ilink/bot/sendmessage", self.base());
        let client_id = format!(
            "pointer-weixin:{}-{}",
            chrono::Utc::now().timestamp_millis(),
            uuid::Uuid::new_v4()
        );
        let body = json!({
            "msg": {
                "from_user_id": "",
                "to_user_id": to_user,
                "client_id": client_id,
                "message_type": 2,
                "message_state": 2,
                "context_token": token,
                "item_list": [{
                    "type": 1,
                    "text_item": { "text": text }
                }]
            },
            "base_info": base_info(),
        });
        let auth = self.auth_headers();
        let headers: Vec<(&str, &str)> = auth
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let resp = self.http.post_json(&url, &headers, &body).await?;
        check_ilink_ret(&resp, "sendmessage")?;
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
        self.http
            .get_json(&url, &[("iLink-App-ClientVersion", "1")])
            .await
    }
}

pub fn parse_login_info(value: &Value) -> Result<WeixinCredentials> {
    let nested = value
        .get("info")
        .or_else(|| value.get("info_json"))
        .unwrap_or(value);
    Ok(WeixinCredentials {
        bot_token: nested
            .get("botToken")
            .or_else(|| nested.get("bot_token"))
            .and_then(|v| v.as_str())
            .context("botToken")?
            .to_string(),
        account_id: nested
            .get("accountId")
            .or_else(|| nested.get("account_id"))
            .or_else(|| nested.get("ilink_bot_id"))
            .and_then(|v| v.as_str())
            .context("accountId")?
            .to_string(),
        base_url: nested
            .get("baseUrl")
            .or_else(|| nested.get("base_url"))
            .or_else(|| nested.get("baseurl"))
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_BASE_URL)
            .to_string(),
        user_id: nested
            .get("userId")
            .or_else(|| nested.get("user_id"))
            .or_else(|| nested.get("ilink_user_id"))
            .and_then(|v| v.as_str())
            .context("userId")?
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_login_info_reads_confirmed_response_fields() {
        let creds = parse_login_info(&json!({
            "status": "confirmed",
            "bot_token": "ilinkbot_test",
            "ilink_bot_id": "bot@im.bot",
            "ilink_user_id": "user@im.wechat",
            "baseurl": "https://ilinkai.weixin.qq.com"
        }))
        .expect("parse");
        assert_eq!(creds.bot_token, "ilinkbot_test");
        assert_eq!(creds.account_id, "bot@im.bot");
        assert_eq!(creds.user_id, "user@im.wechat");
        assert_eq!(creds.base_url, "https://ilinkai.weixin.qq.com");
    }
}
