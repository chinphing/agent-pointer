use anyhow::{anyhow, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};

pub const DEFAULT_API_URL: &str = "https://api.laladama.com";

#[derive(Debug, Clone)]
pub struct DatiConfig {
    pub api_url: String,
    pub authcode: String,
    pub typeno: String,
    pub author: String,
}

impl DatiConfig {
    pub fn from_settings_and_env() -> Self {
        let settings = crate::platform_config::effective_settings_global();
        let env_api_url = std::env::var("DATI_API_URL").unwrap_or_default();
        let env_authcode = std::env::var("DATI_AUTHCODE").unwrap_or_default();
        let env_typeno = std::env::var("DATI_TYPENO").unwrap_or_default();
        let env_author = std::env::var("DATI_AUTHOR").unwrap_or_default();
        Self {
            api_url: first_non_empty(&[
                settings.dati_api_url.as_str(),
                env_api_url.as_str(),
                DEFAULT_API_URL,
            ])
            .trim_end_matches('/')
            .to_string(),
            authcode: first_non_empty(&[settings.dati_authcode.as_str(), env_authcode.as_str()])
                .to_string(),
            typeno: first_non_empty(&[settings.dati_typeno.as_str(), env_typeno.as_str()])
                .to_string(),
            author: first_non_empty(&[settings.dati_author.as_str(), env_author.as_str()])
                .to_string(),
        }
    }

    pub fn validate_for_upload(&self) -> Result<()> {
        if self.authcode.trim().is_empty()
            || self.typeno.trim().is_empty()
            || self.author.trim().is_empty()
        {
            anyhow::bail!(
                "DaTi config missing: set datiAuthcode, datiTypeno, datiAuthor in Settings > Agent > CAPTCHA / DaTi, or env DATI_AUTHCODE, DATI_TYPENO, DATI_AUTHOR."
            );
        }
        Ok(())
    }
}

fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_string()
}

#[derive(Debug, Serialize)]
struct UploadRequest<'a> {
    base64string: &'a str,
    authcode: &'a str,
    typeno: &'a str,
    author: &'a str,
    remark: &'a str,
}

#[derive(Debug, Serialize)]
struct QueryRequest<'a> {
    authcode: &'a str,
    subjectno: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct DatiResponse {
    pub status: i64,
    #[serde(default)]
    pub msg: Value,
}

impl DatiResponse {
    fn msg_text(&self) -> String {
        match &self.msg {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        }
    }
}

pub fn upload(
    client: &Client,
    cfg: &DatiConfig,
    base64string: &str,
    remark: &str,
) -> Result<String> {
    cfg.validate_for_upload()?;
    let payload = UploadRequest {
        base64string,
        authcode: &cfg.authcode,
        typeno: &cfg.typeno,
        author: &cfg.author,
        remark,
    };
    let resp: DatiResponse = client
        .post(format!("{}/member/uploadjson", cfg.api_url))
        .json(&payload)
        .send()
        .map_err(|e| anyhow!("API request failed: {e}"))?
        .error_for_status()
        .map_err(|e| anyhow!("API request failed: {e}"))?
        .json()
        .map_err(|e| anyhow!("Failed to parse response JSON: {e}"))?;
    if resp.status != 0 {
        anyhow::bail!("{}", format_dati_error(resp.status, &resp.msg_text()));
    }
    let subjectno = resp.msg_text();
    if subjectno.trim().is_empty() {
        anyhow::bail!("Upload returned no task id.");
    }
    Ok(subjectno)
}

pub fn query(client: &Client, cfg: &DatiConfig, subjectno: &str) -> Result<DatiResponse> {
    let payload = QueryRequest {
        authcode: &cfg.authcode,
        subjectno,
    };
    client
        .post(format!("{}/member/queryjson", cfg.api_url))
        .json(&payload)
        .send()
        .map_err(|e| anyhow!("API request failed: {e}"))?
        .error_for_status()
        .map_err(|e| anyhow!("API request failed: {e}"))?
        .json()
        .map_err(|e| anyhow!("Failed to parse response JSON: {e}"))
}

pub fn query_until_ready(
    client: &Client,
    cfg: &DatiConfig,
    subjectno: &str,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<String> {
    let deadline = Instant::now() + timeout;
    let mut last: Option<DatiResponse> = None;
    while Instant::now() < deadline {
        let out = query(client, cfg, subjectno)?;
        if out.status == 0 {
            let text = out.msg_text();
            if text.trim().is_empty() {
                anyhow::bail!("Query returned empty answer.");
            }
            return Ok(text);
        }
        if out.status != -100 {
            anyhow::bail!("{}", format_dati_error(out.status, &out.msg_text()));
        }
        last = Some(out);
        std::thread::sleep(poll_interval);
    }
    if let Some(out) = last {
        anyhow::bail!(
            "Answer query timed out after {}s. Last response: status={}, msg={}.",
            timeout.as_secs(),
            out.status,
            out.msg_text()
        );
    }
    anyhow::bail!("Answer query timed out after {}s.", timeout.as_secs());
}

pub fn format_dati_error(status: i64, msg: &str) -> String {
    let base = match status {
        -100 => "Insufficient question credits",
        -101 => "System error (-101)",
        -102 => "System error (-102)",
        -103 => "System error (-103)",
        -104 => "System error (-104)",
        -105 => "System error (-105)",
        -110 => "Missing authcode parameter",
        -111 => "Invalid authcode",
        -112 => "Account for this authcode is disabled",
        -120 => "Developer account does not exist",
        -121 => "Developer account is disabled",
        -130 => "Invalid typeno",
        -131 => "Typeno disabled",
        -132 => "Developer does not match dedicated typeno",
        -133 => "Missing typeno parameter",
        -134 => "Contact support to adjust dedicated typeno pricing",
        -150 => "Image file error (e.g. multiple uploads)",
        -151 => "Image format error (jpg/png/gif/bmp supported)",
        -152 => "Image size exceeds limit (default 1MB)",
        -153 => "Uploaded file is empty",
        -154 => "Server failed to create file",
        -155 => "Server failed to save image file",
        -1 => "Network or local request error",
        _ => return format!("CAPTCHA service error: {status} {msg}"),
    };
    let detail = msg.trim();
    if !detail.is_empty() && detail != base && detail != status.to_string() {
        format!("CAPTCHA service error: {status} {detail}")
    } else {
        format!("CAPTCHA service error: [{status}] {base}")
    }
}
