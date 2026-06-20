//! Resolve media references (`pointer-media://`, storage rel, absolute paths).

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::storage::app_data_dir;

use super::access::{is_user_filesystem_path, normalize_user_path, path_has_traversal};
use super::path_hint::MEDIA_URI_SCHEME;
use super::store::{media_abs_path, CONVERSATION_MEDIA_DIR};

/// Resolve a media reference to an on-disk file path.
pub fn resolve_media_ref(raw: &str) -> Result<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty media ref");
    }

    let rel = trimmed
        .strip_prefix(MEDIA_URI_SCHEME)
        .map(str::trim)
        .unwrap_or(trimmed);

    if !is_user_filesystem_path(rel) && rel.contains('/') {
        let path = media_abs_path(rel)?;
        if path.is_file() {
            return Ok(path);
        }
        anyhow::bail!("media file not found: {trimmed}");
    }

    if path_has_traversal(rel) {
        anyhow::bail!("media path traversal not allowed: {rel}");
    }

    resolve_filesystem_ref(rel)
}

fn resolve_filesystem_ref(raw: &str) -> Result<PathBuf> {
    let path = normalize_user_path(raw)?;
    if path.is_absolute() && path.is_file() {
        return Ok(path);
    }
    if path.is_file() {
        return Ok(path);
    }

    if let Ok(data_dir) = app_data_dir() {
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

    anyhow::bail!("media file not found: {raw}")
}

/// Read raw bytes for a resolved media reference.
pub fn read_media_ref_bytes(raw: &str) -> Result<Vec<u8>> {
    let path = resolve_media_ref(raw)?;
    std::fs::read(&path).with_context(|| format!("read media file {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_ref() {
        assert!(resolve_media_ref("").is_err());
    }

    #[test]
    fn rejects_traversal() {
        assert!(resolve_media_ref("../etc/passwd").is_err());
    }
}
