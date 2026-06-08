use anyhow::{Context, Result};
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use serde_json::json;

const BASE_URL: &str = "https://oapi.dingtalk.com";
const SOURCE: &str = "POINTER_APP";
const REQUEST_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_POLL_INTERVAL_SECS: u64 = 3;
const DEFAULT_EXPIRE_SECS: u64 = 7200;

#[derive(Debug, Clone)]
pub struct DingtalkBeginResult {
    pub device_code: String,
    pub qr_url: String,
    pub interval_secs: u64,
    pub expire_secs: u64,
}

#[derive(Debug, Clone)]
pub struct DingtalkCredentials {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Deserialize)]
struct InitResponse {
    errcode: i64,
    errmsg: String,
    nonce: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BeginResponse {
    errcode: i64,
    errmsg: String,
    device_code: Option<String>,
    verification_uri_complete: Option<String>,
    interval: Option<u64>,
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct PollResponse {
    errcode: i64,
    errmsg: String,
    status: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    fail_reason: Option<String>,
}

pub async fn begin_registration() -> Result<DingtalkBeginResult> {
    let init: InitResponse = post_json("/app/registration/init", json!({ "source": SOURCE })).await?;
    ensure_ok(init.errcode, &init.errmsg)?;
    let nonce = init
        .nonce
        .filter(|s| !s.is_empty())
        .context("dingtalk init: missing nonce")?;

    let begin: BeginResponse =
        post_json("/app/registration/begin", json!({ "nonce": nonce })).await?;
    ensure_ok(begin.errcode, &begin.errmsg)?;

    let device_code = begin
        .device_code
        .filter(|s| !s.is_empty())
        .context("dingtalk begin: missing device_code")?;
    let qr_url = begin
        .verification_uri_complete
        .filter(|s| !s.is_empty())
        .context("dingtalk begin: missing verification_uri_complete")?;

    Ok(DingtalkBeginResult {
        device_code,
        qr_url,
        interval_secs: clamp_secs(begin.interval, DEFAULT_POLL_INTERVAL_SECS),
        expire_secs: clamp_secs(begin.expires_in, DEFAULT_EXPIRE_SECS),
    })
}

pub async fn poll_registration(
    device_code: &str,
    interval_secs: u64,
    expire_secs: u64,
) -> Result<Option<DingtalkCredentials>> {
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(expire_secs.max(60));
    let interval = interval_secs.max(3);

    while std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;

        let poll: PollResponse = match post_json(
            "/app/registration/poll",
            json!({ "device_code": device_code }),
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                log::warn!("dingtalk registration poll transient error: {e:#}");
                continue;
            }
        };
        ensure_ok(poll.errcode, &poll.errmsg)?;

        let status = poll.status.unwrap_or_default().to_uppercase();
        match status.as_str() {
            "WAITING" | "" => {}
            "SUCCESS" => {
                let client_id = poll.client_id.unwrap_or_default().trim().to_string();
                let client_secret = poll.client_secret.unwrap_or_default().trim().to_string();
                if client_id.is_empty() || client_secret.is_empty() {
                    anyhow::bail!("授权成功但凭证为空");
                }
                return Ok(Some(DingtalkCredentials {
                    client_id,
                    client_secret,
                }));
            }
            "FAIL" => {
                let reason = poll
                    .fail_reason
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| poll.errmsg.clone());
                anyhow::bail!("{reason}");
            }
            other => anyhow::bail!("授权失败: {other}"),
        }
    }

    anyhow::bail!("授权超时，请重新扫码")
}

async fn post_json<T: serde::de::DeserializeOwned>(
    path: &str,
    body: serde_json::Value,
) -> Result<T> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(REQUEST_TIMEOUT_MS))
        .build()?;
    let url = format!("{BASE_URL}{path}");
    let res = client
        .post(&url)
        .header(CONTENT_TYPE, "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("dingtalk registration request failed: {url}"))?;
    let text = res.text().await?;
    serde_json::from_str(&text).with_context(|| format!("dingtalk registration invalid json: {text}"))
}

fn ensure_ok(errcode: i64, errmsg: &str) -> Result<()> {
    if errcode != 0 {
        anyhow::bail!("{errmsg} (errcode={errcode})");
    }
    Ok(())
}

fn clamp_secs(value: Option<u64>, default: u64) -> u64 {
    match value {
        Some(v) if v > 0 && v <= 86_400 => v,
        _ => default,
    }
}
