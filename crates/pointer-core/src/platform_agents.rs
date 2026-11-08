//! 控制面账户信息客户端（桌面 Bearer 鉴权）。
//!
//! 云主机商城（`/api/shop/*`、`/api/agents/*`）已下线，这里只保留
//! `/auth/me` —— 余额条与额度门禁依赖它。

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::platform_auth::PlatformAuthManager;

const HTTP_TIMEOUT_SEC: u64 = 60;

fn api_base() -> String {
    crate::platform_endpoints::api_base()
        .trim_end_matches('/')
        .to_string()
}

async fn bearer(auth: &PlatformAuthManager) -> Result<String> {
    auth.ensure_access_token().await
}

async fn parse_error(resp: reqwest::Response) -> anyhow::Error {
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("detail").and_then(|d| d.as_str().map(str::to_string)))
        .unwrap_or_else(|| text.trim().to_string());
    anyhow!("HTTP {status}: {detail}")
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SEC))
        .build()
        .context("build platform http client")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PlatformMe {
    pub balance_yuan: String,
    pub nickname: Option<String>,
    #[serde(default)]
    pub token_quota_exhausted: bool,
}

pub async fn fetch_platform_me(auth: &PlatformAuthManager) -> Result<PlatformMe> {
    let url = format!("{}/auth/me", api_base());
    let client = http_client()?;
    let token = bearer(auth).await?;
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        return Err(parse_error(resp).await);
    }
    resp.json().await.context("parse /auth/me")
}
