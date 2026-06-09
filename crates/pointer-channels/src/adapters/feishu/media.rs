use anyhow::{Context, Result};
use base64::Engine;

use super::auth::{auth_header, guess_mime_from_name, kind_from_mime, normalize_feishu_key, tenant_access_token};
use crate::config::ChannelAccountConfig;
use crate::http_client::HttpClient;
use crate::traits::InboundMediaRef;
use pointer_core::models::MediaAttachment;

pub const FEISHU_MEDIA_MAX_BYTES: usize = 30 * 1024 * 1024;

pub struct DownloadedFeishuMedia {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub file_name: String,
}

pub async fn download_inbound_ref(
    http: &HttpClient,
    account: &ChannelAccountConfig,
    message_id: &str,
    media_ref: &InboundMediaRef,
) -> Result<DownloadedFeishuMedia> {
    let token = tenant_access_token(http, &account.app_id, &account.app_secret).await?;
    let auth = auth_header(&token);
    let headers = [("Authorization", auth.as_str())];

    if let Some(image_key) = media_ref
        .feishu_image_key
        .as_deref()
        .and_then(normalize_feishu_key)
    {
        if media_ref.feishu_file_key.is_none() {
            return download_image(http, &headers, &image_key, media_ref).await;
        }
    }

    let file_key = media_ref
        .feishu_file_key
        .as_deref()
        .or(media_ref.feishu_image_key.as_deref())
        .and_then(normalize_feishu_key)
        .context("feishu media ref missing file_key")?;

    let resource_type = media_ref
        .feishu_resource_type
        .as_deref()
        .unwrap_or("file");
    match download_message_resource(
        http,
        &headers,
        message_id,
        &file_key,
        resource_type,
        media_ref,
    )
    .await
    {
        Ok(v) => Ok(v),
        Err(e) if resource_type == "file" => {
            log::warn!(
                "feishu resource type=file failed for {file_key}: {e:#}; retry type=media"
            );
            download_message_resource(
                http,
                &headers,
                message_id,
                &file_key,
                "media",
                media_ref,
            )
            .await
        }
        Err(e) => Err(e),
    }
}

async fn download_image(
    http: &HttpClient,
    headers: &[(&str, &str)],
    image_key: &str,
    media_ref: &InboundMediaRef,
) -> Result<DownloadedFeishuMedia> {
    let url = format!("https://open.feishu.cn/open-apis/im/v1/images/{image_key}");
    let (bytes, content_type) = http.get_bytes(&url, headers).await?;
    if bytes.len() > FEISHU_MEDIA_MAX_BYTES {
        anyhow::bail!(
            "feishu image exceeds {} MB",
            FEISHU_MEDIA_MAX_BYTES / (1024 * 1024)
        );
    }
    let file_name = media_ref
        .file_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("{image_key}.jpg"));
    let mime_type = content_type
        .or_else(|| media_ref.mime_type.clone())
        .unwrap_or_else(|| guess_mime_from_name(&file_name));
    Ok(DownloadedFeishuMedia {
        bytes,
        mime_type,
        file_name,
    })
}

async fn download_message_resource(
    http: &HttpClient,
    headers: &[(&str, &str)],
    message_id: &str,
    file_key: &str,
    resource_type: &str,
    media_ref: &InboundMediaRef,
) -> Result<DownloadedFeishuMedia> {
    let url = format!(
        "https://open.feishu.cn/open-apis/im/v1/messages/{message_id}/resources/{file_key}?type={resource_type}"
    );
    let (bytes, content_type) = http.get_bytes(&url, headers).await?;
    if bytes.len() > FEISHU_MEDIA_MAX_BYTES {
        anyhow::bail!(
            "feishu resource exceeds {} MB",
            FEISHU_MEDIA_MAX_BYTES / (1024 * 1024)
        );
    }
    let file_name = media_ref
        .file_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("{file_key}.bin"));
    let mime_type = content_type
        .or_else(|| media_ref.mime_type.clone())
        .unwrap_or_else(|| guess_mime_from_name(&file_name));
    Ok(DownloadedFeishuMedia {
        bytes,
        mime_type,
        file_name,
    })
}

pub fn to_media_attachment(downloaded: DownloadedFeishuMedia, kind_hint: &str) -> MediaAttachment {
    let kind = kind_from_mime(&downloaded.mime_type, kind_hint);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&downloaded.bytes);
    MediaAttachment {
        id: uuid::Uuid::new_v4().to_string(),
        kind,
        mime_type: downloaded.mime_type,
        file_name: downloaded.file_name,
        size_bytes: downloaded.bytes.len() as u64,
        storage_rel_path: None,
        content_base64: Some(b64),
        derived_text: None,
    }
}
