use anyhow::{Context, Result};

use super::auth::access_token;
use crate::config::ChannelAccountConfig;
use crate::crypto::wecom_aibot_decrypt_file;
use crate::http_client::HttpClient;
use crate::media::attachment::{enforce_max_bytes_for_kind, DownloadedMedia};
use crate::media::audio_normalize::normalize_channel_audio_download;
use crate::traits::InboundMediaRef;
use pointer_core::media::merge_inbound_filename;

pub const WECOM_AGENT_MEDIA_PREFIX: &str = "agent-media:";

pub fn is_agent_media_ref(media_ref: &InboundMediaRef) -> bool {
    media_ref
        .wecom_download_url
        .as_deref()
        .is_some_and(|u| u.starts_with(WECOM_AGENT_MEDIA_PREFIX))
}

pub async fn download_inbound_ref(
    http: &HttpClient,
    account: Option<&ChannelAccountConfig>,
    media_ref: &InboundMediaRef,
) -> Result<DownloadedMedia> {
    if is_agent_media_ref(media_ref) {
        let account = account.context("wecom agent media download requires account config")?;
        return download_agent_media_ref(http, account, media_ref).await;
    }

    let url = media_ref
        .wecom_download_url
        .as_deref()
        .filter(|s| !s.is_empty())
        .context("wecom media missing url")?;
    let aes_key = media_ref
        .wecom_aes_key
        .as_deref()
        .filter(|s| !s.is_empty())
        .context("wecom media missing aeskey")?;
    let (encrypted, content_type, download_name) = http.get_bytes(url, &[]).await?;
    let file_name = merge_inbound_filename(media_ref.file_name.clone(), download_name)
        .unwrap_or_else(|| format!("wecom-{}.bin", uuid::Uuid::new_v4()));
    let mime_hint = content_type
        .clone()
        .unwrap_or_else(|| "application/octet-stream".into());
    enforce_max_bytes_for_kind(
        &encrypted,
        "wecom media",
        &media_ref.kind,
        &file_name,
        &mime_hint,
    )?;
    let bytes = wecom_aibot_decrypt_file(&encrypted, aes_key).with_context(|| {
        format!(
            "wecom decrypt media url={url} aeskey_len={}",
            aes_key.trim().len()
        )
    })?;
    let mime_type = mime_hint;
    enforce_max_bytes_for_kind(
        &bytes,
        "wecom media",
        &media_ref.kind,
        &file_name,
        &mime_type,
    )?;
    Ok(normalize_channel_audio_download(
        DownloadedMedia {
            bytes,
            mime_type,
            file_name,
        },
        media_ref,
    ))
}

async fn download_agent_media_ref(
    http: &HttpClient,
    account: &ChannelAccountConfig,
    media_ref: &InboundMediaRef,
) -> Result<DownloadedMedia> {
    let url_field = media_ref
        .wecom_download_url
        .as_deref()
        .context("wecom agent media missing url field")?;
    let media_id = url_field
        .strip_prefix(WECOM_AGENT_MEDIA_PREFIX)
        .filter(|s| !s.is_empty())
        .context("wecom agent media id missing")?;
    let corp_id = account.corp_id.trim();
    let secret = account.secret.trim();
    if corp_id.is_empty() || secret.is_empty() {
        anyhow::bail!("wecom agent media download requires corpId and secret");
    }
    let token = access_token(http, corp_id, secret).await?;
    let url = format!(
        "https://qyapi.weixin.qq.com/cgi-bin/media/get?access_token={token}&media_id={media_id}"
    );
    let (bytes, content_type, download_name) = http.get_bytes(&url, &[]).await?;
    let file_name = merge_inbound_filename(media_ref.file_name.clone(), download_name)
        .unwrap_or_else(|| format!("wecom-{media_id}.bin"));
    let mime_type = content_type.unwrap_or_else(|| "application/octet-stream".into());
    enforce_max_bytes_for_kind(
        &bytes,
        "wecom agent media",
        &media_ref.kind,
        &file_name,
        &mime_type,
    )?;
    Ok(normalize_channel_audio_download(
        DownloadedMedia {
            bytes,
            mime_type,
            file_name,
        },
        media_ref,
    ))
}
