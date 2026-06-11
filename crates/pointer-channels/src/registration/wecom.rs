use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;
use std::time::{Duration, Instant};

const BASE_URL: &str = "https://work.weixin.qq.com";
const SOURCE: &str = "POINTER_APP";
const REQUEST_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_POLL_INTERVAL_SECS: u64 = 3;
const DEFAULT_EXPIRE_SECS: u64 = 300;

#[derive(Debug, Clone)]
pub struct WecomBeginResult {
    pub scode: String,
    pub qr_url: String,
    pub interval_secs: u64,
    pub expire_secs: u64,
}

#[derive(Debug, Clone)]
pub struct WecomCredentials {
    pub bot_id: String,
    pub secret: String,
}

#[derive(Debug, Deserialize)]
struct GenSettings {
    scode: String,
    auth_url: String,
}

#[derive(Debug, Deserialize)]
struct QueryResultResponse {
    data: Option<QueryResultData>,
}

#[derive(Debug, Deserialize)]
struct QueryResultData {
    status: Option<String>,
    bot_info: Option<BotInfo>,
}

#[derive(Debug, Deserialize)]
struct BotInfo {
    botid: Option<String>,
    secret: Option<String>,
}

pub async fn begin_registration() -> Result<WecomBeginResult> {
    let state = generate_state();
    let timestamp = chrono::Utc::now().timestamp_millis();
    let url = format!(
        "{BASE_URL}/ai/qc/gen?source={SOURCE}&state={state}&timestamp={timestamp}"
    );
    let html = get_text(&url).await?;
    let settings = parse_gen_settings(&html)?;
    if settings.scode.trim().is_empty() || settings.auth_url.trim().is_empty() {
        anyhow::bail!("wecom gen: missing scode or auth_url");
    }
    Ok(WecomBeginResult {
        scode: settings.scode,
        qr_url: settings.auth_url,
        interval_secs: DEFAULT_POLL_INTERVAL_SECS,
        expire_secs: DEFAULT_EXPIRE_SECS,
    })
}

pub async fn poll_registration(
    scode: &str,
    interval_secs: u64,
    expire_secs: u64,
) -> Result<Option<WecomCredentials>> {
    let deadline = Instant::now() + Duration::from_secs(expire_secs.max(60));
    let interval = interval_secs.max(3);

    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_secs(interval)).await;

        let url = format!(
            "{BASE_URL}/ai/qc/query_result?scode={}",
            urlencoding::encode(scode)
        );
        let body = match get_text(&url).await {
            Ok(v) => v,
            Err(e) => {
                log::warn!("wecom registration poll transient error: {e:#}");
                continue;
            }
        };

        let parsed: QueryResultResponse =
            serde_json::from_str(&body).with_context(|| format!("wecom poll invalid json: {body}"))?;
        let Some(data) = parsed.data else {
            continue;
        };
        match data.status.as_deref() {
            Some("success") => {
                let bot_info = data.bot_info.context("wecom poll: missing bot_info")?;
                let bot_id = bot_info
                    .botid
                    .filter(|s| !s.trim().is_empty())
                    .context("wecom poll: missing botid")?;
                let secret = bot_info
                    .secret
                    .filter(|s| !s.trim().is_empty())
                    .context("wecom poll: missing secret")?;
                return Ok(Some(WecomCredentials { bot_id, secret }));
            }
            Some("init") | Some("pending") | None => continue,
            Some(other) => anyhow::bail!("wecom poll unexpected status: {other}"),
        }
    }

    anyhow::bail!("授权超时，请重新扫码")
}

fn parse_gen_settings(html: &str) -> Result<GenSettings> {
    let marker = "window.settings = ";
    let start = html
        .find(marker)
        .context("wecom gen: window.settings not found")?;
    let json_start = start + marker.len();
    let rest = html[json_start..].trim_start();
    let json_str = extract_json_object(rest).context("wecom gen: settings json object not found")?;
    serde_json::from_str(json_str).with_context(|| format!("wecom gen: invalid settings json: {json_str}"))
}

/// Extract the first `{...}` object from HTML/JS snippet (stops at balanced `}`).
fn extract_json_object(s: &str) -> Result<&str> {
    let start = s.find('{').context("json object start not found")?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escape = false;
    for (offset, ch) in s[start..].char_indices() {
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = start + offset + 1;
                    return Ok(&s[start..end]);
                }
            }
            _ => {}
        }
    }
    anyhow::bail!("unterminated json object")
}

fn generate_state() -> String {
    format!(
        "state_{}_{}",
        rand::random::<u32>(),
        chrono::Utc::now().timestamp_millis()
    )
}

async fn get_text(url: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(REQUEST_TIMEOUT_MS))
        .default_headers(default_headers())
        .build()
        .context("wecom http client")?;
    let text = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("wecom request failed: {url}"))?
        .error_for_status()
        .with_context(|| format!("wecom request status error: {url}"))?
        .text()
        .await
        .with_context(|| format!("wecom response body failed: {url}"))?;
    Ok(text)
}

fn default_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
        ),
    );
    headers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gen_settings_extracts_scode_and_auth_url() {
        let html = r#"<!DOCTYPE html><script>window.settings = {"NODE_ENV":"production","scode":"abc-123","auth_url":"https://work.weixin.qq.com/ai/qc/c?s=abc-123&hide_more_btn=true&for_native=true"}</script><script>window.__LQ_REPORT_URL__ = '/ai/report'</script><script>//#ignore_check_debug window.DEBUG_TRUE = true; window.DEBUG_FALSE = false</script>"#;
        let parsed = parse_gen_settings(html).expect("parse");
        assert_eq!(parsed.scode, "abc-123");
        assert!(parsed.auth_url.contains("abc-123"));
    }
}
