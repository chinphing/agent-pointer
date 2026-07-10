//! Resolve outbound media path references to readable local files.

use anyhow::{Context, Result};
use pointer_core::media::path_hint::MEDIA_URI_SCHEME;
use pointer_core::media::resolve::resolve_local_media_path;
use pointer_core::media::store::path_under_app_data;
use pointer_core::media::is_user_filesystem_path;
use pointer_core::media::{is_video_file_name, video::remux_video_faststart};

use crate::media::attachment::{enforce_max_bytes_for_kind, guess_mime_from_bytes};
use crate::traits::OutboundMedia;

/// Outbound kind hint for size limits (video uses Composer OSS ceiling).
fn outbound_media_kind(mime_type: &str, file_name: &str) -> &'static str {
    let mime = mime_type.trim().to_ascii_lowercase();
    if mime.starts_with("video/") || is_video_file_name(file_name) {
        "video"
    } else {
        "file"
    }
}

/// MP4/MOV containers benefit from `-movflags +faststart`; other video types are skipped.
fn should_remux_for_im_preview(mime_type: &str, file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    matches!(mime_type, "video/mp4" | "video/quicktime")
        || lower.ends_with(".mp4")
        || lower.ends_with(".mov")
}

pub struct ResolvedOutboundMedia {
    pub media: OutboundMedia,
}

pub fn resolve_outbound_media(raw: &str) -> Result<ResolvedOutboundMedia> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty media path");
    }

    let rel = trimmed
        .strip_prefix(MEDIA_URI_SCHEME)
        .map(str::trim)
        .unwrap_or(trimmed);

    let path = resolve_local_media_path(rel)
        .with_context(|| format!("resolve outbound media {trimmed}"))?;
    if path.is_dir() {
        anyhow::bail!("outbound media path is a directory: {}", path.display());
    }
    if !is_user_filesystem_path(rel)
        && !path_under_app_data(&path).unwrap_or(false)
    {
        anyhow::bail!(
            "outbound media path outside app data (server cannot deliver): {}",
            path.display()
        );
    }

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment.bin")
        .to_string();
    let bytes = std::fs::read(&path)
        .with_context(|| format!("read outbound media {}", path.display()))?;

    let mut mime_type = mime_guess::from_path(&file_name)
        .first()
        .map(|m| m.essence_str().to_string())
        .unwrap_or_else(|| "application/octet-stream".into());
    if mime_type == "application/octet-stream" {
        if let Some(m) = guess_mime_from_bytes(&bytes) {
            mime_type = m.into();
        }
    }

    let kind = outbound_media_kind(&mime_type, &file_name);
    enforce_max_bytes_for_kind(&bytes, "outbound media", kind, &file_name, &mime_type)?;

    log::info!(
        "outbound media resolved path={} bytes={} mime={}",
        path.display(),
        bytes.len(),
        mime_type
    );

    // AI-generated videos often have the moov atom at the end, causing IM platforms
    // (Feishu, etc.) to show "0s" duration in previews. Remux with +faststart to move
    // the moov atom to the beginning — a fast stream-copy, no re-encoding.
    let bytes = if should_remux_for_im_preview(&mime_type, &file_name) {
        remux_video_faststart(&bytes, &file_name)
    } else {
        bytes
    };

    Ok(ResolvedOutboundMedia {
        media: OutboundMedia {
            file_name,
            mime_type,
            bytes,
            local_path: Some(path),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_path() {
        assert!(resolve_outbound_media("").is_err());
    }

    #[test]
    fn rejects_traversal() {
        assert!(resolve_outbound_media("../secret.pdf").is_err());
    }

    #[test]
    fn resolves_file_uri_as_local_path() {
        let file = std::env::temp_dir().join(format!(
            "pointer-outbound-resolve-{}.txt",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&file, b"hello").unwrap();
        let uri = format!("file://{}", file.display());
        let resolved = resolve_outbound_media(&uri).expect("resolve file uri");
        assert_eq!(resolved.media.bytes, b"hello");
        let _ = std::fs::remove_file(&file);
    }
}
