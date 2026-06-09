use base64::Engine;
use pointer_core::models::MediaAttachment;

use crate::adapters::feishu::auth::{guess_mime_from_name, kind_from_mime};

pub struct DownloadedMedia {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub file_name: String,
}

pub fn to_media_attachment(downloaded: DownloadedMedia, kind_hint: &str) -> MediaAttachment {
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

pub fn finalize_downloaded(
    mut downloaded: DownloadedMedia,
    file_name: Option<String>,
) -> DownloadedMedia {
    if let Some(name) = file_name.filter(|s| !s.trim().is_empty()) {
        downloaded.file_name = name;
    }
    if downloaded.mime_type == "application/octet-stream" {
        downloaded.mime_type = guess_mime_from_name(&downloaded.file_name);
    }
    downloaded
}

pub const CHANNEL_MEDIA_MAX_BYTES: usize = 30 * 1024 * 1024;

pub fn enforce_max_bytes(bytes: &[u8], label: &str) -> anyhow::Result<()> {
    if bytes.len() > CHANNEL_MEDIA_MAX_BYTES {
        anyhow::bail!(
            "{label} exceeds {} MB",
            CHANNEL_MEDIA_MAX_BYTES / (1024 * 1024)
        );
    }
    Ok(())
}
