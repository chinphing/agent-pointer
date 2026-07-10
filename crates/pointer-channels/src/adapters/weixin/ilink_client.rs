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

#[derive(Debug, Clone, Copy)]
pub enum GetConfigContextToken<'a> {
    /// Do not include `context_token` in the request body.
    Omit,
    /// Include `"context_token": ""` (some sessions only refresh with an explicit empty field).
    Empty,
    /// Include `"context_token": "<value>"`.
    Value(&'a str),
}

impl GetConfigContextToken<'_> {
    pub fn log_label(self) -> &'static str {
        match self {
            Self::Omit => "omit",
            Self::Empty => "empty",
            Self::Value(_) => "value",
        }
    }
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

fn auth_header_refs(auth: &[(String, String)]) -> Vec<(&str, &str)> {
    auth.iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect()
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
        let headers = auth_header_refs(&auth);
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
        let mut items = vec![json!({ "type": 1, "text_item": { "text": text } })];
        self.send_message_items(to_user, token, &mut items).await
    }

    pub async fn send_message_items(
        &self,
        to_user: &str,
        context_token: &str,
        item_list: &mut [Value],
    ) -> Result<()> {
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
                "context_token": context_token,
                "item_list": item_list
            },
            "base_info": base_info(),
        });
        let auth = self.auth_headers();
        let headers = auth_header_refs(&auth);
        let resp = self.http.post_json(&url, &headers, &body).await?;
        check_ilink_ret(&resp, "sendmessage")?;
        Ok(())
    }

    /// Fetch bot config; may return a refreshed `context_token` when the current one is stale.
    pub async fn get_config(
        &self,
        ilink_user_id: &str,
        context_token: GetConfigContextToken<'_>,
    ) -> Result<Value> {
        let url = format!("{}/ilink/bot/getconfig", self.base());
        let mut body = json!({
            "ilink_user_id": ilink_user_id,
            "base_info": base_info(),
        });
        if let Some(obj) = body.as_object_mut() {
            match context_token {
                GetConfigContextToken::Omit => {}
                GetConfigContextToken::Empty => {
                    obj.insert("context_token".into(), json!(""));
                }
                GetConfigContextToken::Value(token) => {
                    obj.insert("context_token".into(), json!(token));
                }
            }
        }
        let auth = self.auth_headers();
        let headers = auth_header_refs(&auth);
        let resp = self.http.post_json(&url, &headers, &body).await?;
        check_ilink_ret(&resp, "getconfig")?;
        Ok(resp)
    }

    pub async fn get_upload_url(
        &self,
        to_user_id: &str,
        filekey: &str,
        media_type: u64,
        rawsize: u64,
        filesize: u64,
        rawfilemd5: &str,
        aeskey_hex: &str,
    ) -> Result<Value> {
        let url = format!("{}/ilink/bot/getuploadurl", self.base());
        let body = json!({
            "filekey": filekey,
            "media_type": media_type,
            "to_user_id": to_user_id,
            "rawsize": rawsize,
            "rawfilemd5": rawfilemd5,
            "filesize": filesize,
            "aeskey": aeskey_hex,
            "no_need_thumb": true,
            "base_info": base_info(),
        });
        let auth = self.auth_headers();
        let headers = auth_header_refs(&auth);
        let resp = self.http.post_json(&url, &headers, &body).await?;
        check_ilink_ret(&resp, "getuploadurl")?;
        Ok(resp)
    }

    pub async fn post_cdn_bytes(
        &self,
        upload_url: &str,
        body: &[u8],
    ) -> Result<(Value, Vec<(String, String)>)> {
        self.http
            .post_bytes(upload_url, &[], body, "application/octet-stream")
            .await
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
