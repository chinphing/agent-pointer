use anyhow::{Context, Result};

use super::auth::{
    auth_header, guess_mime_from_name, normalize_feishu_key, tenant_access_token,
};
use crate::config::ChannelAccountConfig;
use crate::http_client::HttpClient;
use crate::media::attachment::{enforce_max_bytes_for_kind, DownloadedMedia};
use crate::media::audio_normalize::normalize_channel_audio_download;
use crate::traits::InboundMediaRef;
use pointer_core::models::MediaAttachment;

pub async fn download_inbound_ref(
    http: &HttpClient,
    account: &ChannelAccountConfig,
    message_id: &str,
    media_ref: &InboundMediaRef,
) -> Result<DownloadedMedia> {
    let token = tenant_access_token(http, &account.app_id, &account.app_secret).await?;
    let auth = auth_header(&token);
    let headers = [("Authorization", auth.as_str())];

    let downloaded = if let Some(image_key) = media_ref
        .feishu_image_key
        .as_deref()
        .and_then(normalize_feishu_key)
    {
        if media_ref.feishu_file_key.is_none() {
            download_image(http, &headers, &image_key, media_ref).await?
        } else {
            download_file_resource(http, &headers, message_id, media_ref).await?
        }
    } else {
        download_file_resource(http, &headers, message_id, media_ref).await?
    };

    Ok(normalize_channel_audio_download(downloaded, media_ref))
}

async fn download_file_resource(
    http: &HttpClient,
    headers: &[(&str, &str)],
    message_id: &str,
    media_ref: &InboundMediaRef,
) -> Result<DownloadedMedia> {
    let file_key = media_ref
        .feishu_file_key
        .as_deref()
        .or(media_ref.feishu_image_key.as_deref())
        .and_then(normalize_feishu_key)
        .context("feishu media ref missing file_key")?;

    let api_type = feishu_download_resource_type(media_ref);
    match download_message_resource(
        http,
        headers,
        message_id,
        &file_key,
        api_type,
        media_ref,
    )
    .await
    {
        Ok(v) => Ok(v),
        Err(e) if api_type == "file" => {
            log::warn!(
                "feishu resource type=file failed for {file_key}: {e:#}; retry type=media"
            );
            download_message_resource(
                http,
                headers,
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

/// Feishu `message-resource` API only supports `type=image` or `type=file`.
/// Audio and video must use `type=file` (not `type=audio`).
fn feishu_download_resource_type(media_ref: &InboundMediaRef) -> &'static str {
    let declared = media_ref
        .feishu_resource_type
        .as_deref()
        .unwrap_or("file")
        .trim()
        .to_ascii_lowercase();
    if declared == "image" {
        return "image";
    }
    "file"
}

async fn download_image(
    http: &HttpClient,
    headers: &[(&str, &str)],
    image_key: &str,
    media_ref: &InboundMediaRef,
) -> Result<DownloadedMedia> {
    let url = format!("https://open.feishu.cn/open-apis/im/v1/images/{image_key}");
    let (bytes, content_type, download_name) = http.get_bytes(&url, headers).await?;
    let file_name = media_ref
        .file_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or(download_name)
        .unwrap_or_else(|| format!("{image_key}.jpg"));
    let mime_type = content_type
        .or_else(|| media_ref.mime_type.clone())
        .unwrap_or_else(|| guess_mime_from_name(&file_name));
    enforce_max_bytes_for_kind(
        &bytes,
        "feishu image",
        &media_ref.kind,
        &file_name,
        &mime_type,
    )?;
    Ok(DownloadedMedia {
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
) -> Result<DownloadedMedia> {
    let url = format!(
        "https://open.feishu.cn/open-apis/im/v1/messages/{message_id}/resources/{file_key}?type={resource_type}"
    );
    let (bytes, content_type, download_name) = http.get_bytes(&url, headers).await?;
    let file_name = media_ref
        .file_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or(download_name)
        .unwrap_or_else(|| format!("{file_key}.bin"));
    let mime_type = content_type
        .or_else(|| media_ref.mime_type.clone())
        .unwrap_or_else(|| guess_mime_from_name(&file_name));
    enforce_max_bytes_for_kind(
        &bytes,
        "feishu resource",
        &media_ref.kind,
        &file_name,
        &mime_type,
    )?;
    Ok(DownloadedMedia {
        bytes,
        mime_type,
        file_name,
    })
}

pub fn to_media_attachment(downloaded: DownloadedMedia, kind_hint: &str) -> MediaAttachment {
    crate::media::attachment::to_media_attachment(downloaded, kind_hint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::InboundMediaRef;

    fn audio_ref(resource_type: Option<&str>) -> InboundMediaRef {
        InboundMediaRef {
            kind: "audio".into(),
            mime_type: Some("audio/ogg".into()),
            file_name: None,
            feishu_image_key: None,
            feishu_file_key: Some("file_v3_abc".into()),
            feishu_resource_type: resource_type.map(|s| s.into()),
            wecom_download_url: None,
            wecom_aes_key: None,
            dingtalk_download_code: None,
            weixin_encrypt_query_param: None,
            weixin_aes_key: None,
            weixin_image_aeskey_hex: None,
            weixin_voice_encode_type: None,
            weixin_voice_sample_rate: None,
            weixin_voice_asr_text: None,
        }
    }

    #[test]
    fn audio_download_uses_file_type() {
        assert_eq!(feishu_download_resource_type(&audio_ref(Some("audio"))), "file");
        assert_eq!(feishu_download_resource_type(&audio_ref(Some("file"))), "file");
        assert_eq!(feishu_download_resource_type(&audio_ref(None)), "file");
    }
}
