use anyhow::{Context, Result};

use crate::crypto::wecom_aibot_decrypt_file;
use crate::http_client::HttpClient;
use crate::media::attachment::{enforce_max_bytes, finalize_downloaded, DownloadedMedia};
use crate::traits::InboundMediaRef;

pub async fn download_inbound_ref(
    http: &HttpClient,
    media_ref: &InboundMediaRef,
) -> Result<DownloadedMedia> {
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
    let (encrypted, content_type) = http.get_bytes(url, &[]).await?;
    enforce_max_bytes(&encrypted, "wecom media")?;
    let bytes = wecom_aibot_decrypt_file(&encrypted, aes_key)
        .with_context(|| format!("wecom decrypt media url={url}"))?;
    enforce_max_bytes(&bytes, "wecom media")?;
    let file_name = media_ref
        .file_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("wecom-{}.bin", uuid::Uuid::new_v4()));
    Ok(finalize_downloaded(
        DownloadedMedia {
            bytes,
            mime_type: content_type.unwrap_or_else(|| "application/octet-stream".into()),
            file_name,
        },
        media_ref.file_name.clone(),
    ))
}
