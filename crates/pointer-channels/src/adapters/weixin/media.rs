use anyhow::{Context, Result};

use super::cdn::{download_and_decrypt, WEIXIN_CDN_BASE};
use super::silk::normalize_weixin_voice_bytes;
use crate::adapters::feishu::auth::guess_mime_from_name;
use crate::http_client::HttpClient;
use crate::media::attachment::{enforce_max_bytes, finalize_downloaded, DownloadedMedia};
use crate::traits::InboundMediaRef;
use pointer_core::models::MediaAttachment;

pub async fn download_inbound_ref(
    http: &HttpClient,
    media_ref: &InboundMediaRef,
) -> Result<DownloadedMedia> {
    let encrypt_query_param = media_ref
        .weixin_encrypt_query_param
        .as_deref()
        .filter(|s| !s.is_empty())
        .context("weixin media missing encrypt_query_param")?;
    let mut bytes = download_and_decrypt(
        http,
        encrypt_query_param,
        media_ref.weixin_aes_key.as_deref(),
        media_ref.weixin_image_aeskey_hex.as_deref(),
    )
    .await?;
    enforce_max_bytes(&bytes, "weixin media")?;

    let file_name = media_ref
        .file_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("weixin-{}.bin", uuid::Uuid::new_v4()));

    if media_ref.kind == "audio" {
        match normalize_weixin_voice_bytes(
            &bytes,
            media_ref.weixin_voice_encode_type,
            media_ref.weixin_voice_sample_rate,
        ) {
            Ok((wav, mime)) => {
                bytes = wav;
                let out_name = if file_name.ends_with(".wav") {
                    file_name
                } else {
                    file_name.rsplit_once('.').map(|(b, _)| format!("{b}.wav")).unwrap_or_else(|| format!("{file_name}.wav"))
                };
                return Ok(finalize_downloaded(
                    DownloadedMedia {
                        bytes,
                        mime_type: mime,
                        file_name: out_name,
                    },
                    None,
                ));
            }
            Err(e) => {
                log::warn!(
                    "weixin silk decode failed kind=audio name={file_name}: {e:#}; keeping raw bytes"
                );
            }
        }
    }

    let mime_type = media_ref
        .mime_type
        .clone()
        .unwrap_or_else(|| guess_mime_from_name(&file_name));
    Ok(finalize_downloaded(
        DownloadedMedia {
            bytes,
            mime_type,
            file_name,
        },
        media_ref.file_name.clone(),
    ))
}

pub fn to_media_attachment(
    downloaded: DownloadedMedia,
    media_ref: &InboundMediaRef,
    kind_hint: &str,
) -> MediaAttachment {
    let mut att = crate::media::attachment::to_media_attachment(downloaded, kind_hint);
    if let Some(asr) = media_ref
        .weixin_voice_asr_text
        .as_ref()
        .filter(|s| !s.trim().is_empty())
    {
        att.derived_text = Some(asr.clone());
    }
    att
}

pub fn cdn_download_url(encrypt_query_param: &str) -> String {
    format!(
        "{WEIXIN_CDN_BASE}/download?encrypted_query_param={}",
        urlencoding::encode(encrypt_query_param)
    )
}
