//! Resolve outbound media path references to readable local files.

use std::path::PathBuf;

use anyhow::{Context, Result};
use pointer_core::media::store::{media_abs_path, read_media_bytes, CONVERSATION_MEDIA_DIR};
use pointer_core::media::path_hint::MEDIA_URI_SCHEME;
use pointer_core::media::video::remux_video_faststart;

use crate::media::attachment::{enforce_max_bytes, guess_mime_from_bytes};
use crate::media_roots::{is_user_filesystem_path, normalize_user_path, path_has_traversal};
use crate::traits::OutboundMedia;

const MAX_OUTBOUND_MEDIA: usize = 30 * 1024 * 1024;

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

    let (bytes, file_name, source_path) = if !is_user_filesystem_path(rel) && rel.contains('/') {
        let abs = media_abs_path(rel).with_context(|| format!("resolve pointer-media {rel}"))?;
        let name = abs
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment.bin")
            .to_string();
        let bytes = read_media_bytes(rel)?;
        (bytes, name, abs)
    } else {
        if path_has_traversal(rel) {
            anyhow::bail!("outbound media path traversal not allowed: {rel}");
        }
        let path = resolve_filesystem_path(rel)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment.bin")
            .to_string();
        let bytes = std::fs::read(&path)
            .with_context(|| format!("read outbound media {}", path.display()))?;
        (bytes, name, path)
    };

    enforce_max_bytes(&bytes, "outbound media")?;
    if bytes.len() > MAX_OUTBOUND_MEDIA {
        anyhow::bail!(
            "outbound media exceeds {} MB",
            MAX_OUTBOUND_MEDIA / (1024 * 1024)
        );
    }

    let mut mime_type = mime_guess::from_path(&file_name)
        .first()
        .map(|m| m.essence_str().to_string())
        .unwrap_or_else(|| "application/octet-stream".into());
    if mime_type == "application/octet-stream" {
        if let Some(m) = guess_mime_from_bytes(&bytes) {
            mime_type = m.into();
        }
    }

    log::info!(
        "outbound media resolved path={} bytes={} mime={}",
        source_path.display(),
        bytes.len(),
        mime_type
    );

    // AI-generated videos often have the moov atom at the end, causing IM platforms
    // (Feishu, etc.) to show "0s" duration in previews. Remux with +faststart to move
    // the moov atom to the beginning — a fast stream-copy, no re-encoding.
    let bytes = if mime_type.starts_with("video/") {
        remux_video_faststart(&bytes, &file_name)
    } else {
        bytes
    };

    Ok(ResolvedOutboundMedia {
        media: OutboundMedia {
            file_name,
            mime_type,
            bytes,
            local_path: Some(source_path),
        },
    })
}

fn resolve_filesystem_path(raw: &str) -> Result<PathBuf> {
    let path = normalize_user_path(raw)?;
    if path.is_absolute() {
        return Ok(path);
    }

    if let Ok(data_dir) = pointer_core::storage::app_data_dir() {
        let under_data = data_dir.join(raw.trim_start_matches('/'));
        if under_data.is_file() {
            return Ok(under_data);
        }
        let under_media = data_dir
            .join(CONVERSATION_MEDIA_DIR)
            .join(raw.trim_start_matches('/'));
        if under_media.is_file() {
            return Ok(under_media);
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let under_cwd = cwd.join(raw);
        if under_cwd.is_file() {
            return Ok(under_cwd);
        }
    }

    anyhow::bail!("outbound media path not found: {raw}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_path() {
        assert!(resolve_outbound_media("").is_err());
    }

    #[test]
    fn resolves_file_uri_as_local_path() {
        let file = std::env::temp_dir().join(format!(
            "pointer_outbound_index_test_{}.html",
            std::process::id()
        ));
        std::fs::write(&file, b"<html></html>").unwrap();
        let uri = format!("file://{}", file.display());
        let resolved = resolve_outbound_media(&uri).unwrap();
        assert!(resolved.media.file_name.ends_with(".html"));
        assert_eq!(resolved.media.bytes, b"<html></html>");
        let _ = std::fs::remove_file(&file);
    }
}
