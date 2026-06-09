//! Resolve outbound media path references to readable local files.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use pointer_core::media::store::{media_abs_path, read_media_bytes, CONVERSATION_MEDIA_DIR};
use pointer_core::media::path_hint::MEDIA_URI_SCHEME;

use crate::media::attachment::{enforce_max_bytes, guess_mime_from_bytes};
use crate::media_roots::{
    assert_media_path_allowed, is_pointer_media_uri, is_user_filesystem_path, normalize_user_path,
    path_has_traversal,
};
use crate::traits::OutboundMedia;

const MAX_OUTBOUND_MEDIA: usize = 30 * 1024 * 1024;

pub struct ResolvedOutboundMedia {
    pub media: OutboundMedia,
}

pub fn resolve_outbound_media(raw: &str) -> Result<ResolvedOutboundMedia> {
    resolve_outbound_media_with_policy(raw, &[])
}

pub fn resolve_outbound_media_with_policy(
    raw: &str,
    media_local_roots: &[String],
) -> Result<ResolvedOutboundMedia> {
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
        if !is_pointer_media_uri(raw) {
            assert_media_path_allowed(&path, media_local_roots)?;
        }
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
}
