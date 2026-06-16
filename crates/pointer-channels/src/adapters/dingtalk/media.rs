use anyhow::{Context, Result};

use super::auth::access_token;
use crate::adapters::feishu::auth::guess_mime_from_name;
use crate::config::ChannelAccountConfig;
use crate::http_client::HttpClient;
use crate::media::attachment::{enforce_max_bytes, DownloadedMedia};
use crate::media::audio_normalize::normalize_channel_audio_download;
use crate::traits::InboundMediaRef;
use serde_json::json;

const DOWNLOAD_URL: &str = "https://api.dingtalk.com/v1.0/robot/messageFiles/download";

pub async fn download_inbound_ref(
    http: &HttpClient,
    account: &ChannelAccountConfig,
    media_ref: &InboundMediaRef,
) -> Result<DownloadedMedia> {
    let download_code = media_ref
        .dingtalk_download_code
        .as_deref()
        .filter(|s| !s.is_empty())
        .context("dingtalk media missing downloadCode")?;
    let robot_code = account
        .client_id
        .trim()
        .to_string();
    if robot_code.is_empty() {
        anyhow::bail!("dingtalk account missing clientId (robotCode)");
    }
    let token = access_token(http, &account.client_id, &account.client_secret).await?;
    let headers = [
        ("x-acs-dingtalk-access-token", token.as_str()),
        ("Content-Type", "application/json"),
    ];
    let body = json!({
        "downloadCode": download_code,
        "robotCode": robot_code,
    });
    let resp = http.post_json(DOWNLOAD_URL, &headers, &body).await?;
    let download_url = resp
        .get("downloadUrl")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .context("dingtalk downloadUrl missing")?;
    let (bytes, content_type, download_name) = http.get_bytes(download_url, &[]).await?;
    enforce_max_bytes(&bytes, "dingtalk media")?;
    let file_name = media_ref
        .file_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or(download_name)
        .unwrap_or_else(|| format!("dingtalk-{}.bin", uuid::Uuid::new_v4()));
    let mime_type = content_type
        .or_else(|| media_ref.mime_type.clone())
        .unwrap_or_else(|| guess_mime_from_name(&file_name));
    Ok(normalize_channel_audio_download(
        DownloadedMedia {
            bytes,
            mime_type,
            file_name,
        },
        media_ref,
    ))
}
