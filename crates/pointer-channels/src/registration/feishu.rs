use anyhow::{Context, Result};
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use url::Url;

const FEISHU_ACCOUNTS_URL: &str = "https://accounts.feishu.cn";
const REGISTRATION_PATH: &str = "/oauth/v1/app/registration";
const REQUEST_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_POLL_INTERVAL_SECS: u64 = 5;
const DEFAULT_EXPIRE_SECS: u64 = 600;

#[derive(Debug, Clone)]
pub struct FeishuBeginResult {
    pub device_code: String,
    pub qr_url: String,
    pub interval_secs: u64,
    pub expire_secs: u64,
}

#[derive(Debug, Clone)]
pub struct FeishuCredentials {
    pub app_id: String,
    pub app_secret: String,
}

#[derive(Debug, Deserialize)]
struct InitResponse {
    supported_auth_methods: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct BeginResponse {
    device_code: String,
    verification_uri_complete: String,
    interval: Option<u64>,
    expire_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct PollResponse {
    client_id: Option<String>,
    client_secret: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub async fn init_registration() -> Result<()> {
    let res: InitResponse = post_form(FEISHU_ACCOUNTS_URL, &[("action", "init")]).await?;
    let methods = res.supported_auth_methods.unwrap_or_default();
    if !methods.iter().any(|m| m == "client_secret") {
        anyhow::bail!("当前环境不支持 client_secret 授权方式");
    }
    Ok(())
}

pub async fn begin_registration() -> Result<FeishuBeginResult> {
    init_registration().await?;
    let res: BeginResponse = post_form(
        FEISHU_ACCOUNTS_URL,
        &[
            ("action", "begin"),
            ("archetype", "PersonalAgent"),
            ("auth_method", "client_secret"),
            ("request_user_info", "open_id"),
        ],
    )
    .await?;

    let mut qr_url = Url::parse(&res.verification_uri_complete)
        .context("invalid feishu verification_uri_complete")?;
    {
        let mut pairs = qr_url.query_pairs_mut();
        pairs.append_pair("from", "oc_onboard");
        pairs.append_pair("tp", "ob_cli_app");
    }

    Ok(FeishuBeginResult {
        device_code: res.device_code,
        qr_url: qr_url.to_string(),
        interval_secs: clamp_secs(res.interval, DEFAULT_POLL_INTERVAL_SECS),
        expire_secs: clamp_secs(res.expire_in, DEFAULT_EXPIRE_SECS),
    })
}

pub async fn poll_registration(
    device_code: &str,
    interval_secs: u64,
    expire_secs: u64,
) -> Result<Option<FeishuCredentials>> {
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(expire_secs.max(60));
    let mut interval = interval_secs.max(3);

    while std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;

        let poll: PollResponse = match post_form(
            FEISHU_ACCOUNTS_URL,
            &[
                ("action", "poll"),
                ("device_code", device_code),
                ("tp", "ob_cli_app"),
            ],
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                log::warn!("feishu registration poll transient error: {e:#}");
                continue;
            }
        };

        if let (Some(app_id), Some(app_secret)) = (poll.client_id, poll.client_secret) {
            if !app_id.is_empty() && !app_secret.is_empty() {
                return Ok(Some(FeishuCredentials { app_id, app_secret }));
            }
        }

        match poll.error.as_deref() {
            None | Some("authorization_pending") => {}
            Some("slow_down") => interval = interval.saturating_add(5),
            Some("access_denied") => anyhow::bail!("用户拒绝授权"),
            Some("expired_token") => anyhow::bail!("授权已过期，请重新扫码"),
            Some(other) => {
                let desc = poll.error_description.unwrap_or_default();
                anyhow::bail!("{other}: {desc}");
            }
        }
    }

    anyhow::bail!("授权超时，请重新扫码")
}

async fn post_form<T: serde::de::DeserializeOwned>(
    base_url: &str,
    fields: &[(&str, &str)],
) -> Result<T> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(REQUEST_TIMEOUT_MS))
        .build()?;
    let url = format!("{base_url}{REGISTRATION_PATH}");
    let res = client
        .post(&url)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(form_body(fields))
        .send()
        .await
        .with_context(|| format!("feishu registration request failed: {url}"))?;
    let text = res.text().await?;
    serde_json::from_str(&text).with_context(|| format!("feishu registration invalid json: {text}"))
}

fn form_body(fields: &[(&str, &str)]) -> String {
    fields
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn clamp_secs(value: Option<u64>, default: u64) -> u64 {
    match value {
        Some(v) if v > 0 && v <= 86_400 => v,
        _ => default,
    }
}
