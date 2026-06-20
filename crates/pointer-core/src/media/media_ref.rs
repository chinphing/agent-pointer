//! Resolve media references (`pointer-media://`, storage rel, absolute paths).

use std::path::PathBuf;

use anyhow::{Context, Result};

use super::path_hint::MEDIA_URI_SCHEME;
use super::resolve::resolve_local_media_path;

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

    resolve_local_media_path(rel).with_context(|| format!("resolve media ref {trimmed}"))
}

/// True when `raw` resolves to an existing directory.
pub fn media_ref_is_directory(raw: &str) -> Result<bool> {
    Ok(resolve_media_ref(raw)?.is_dir())
}

/// Read raw bytes for a resolved media reference (files only).
pub fn read_media_ref_bytes(raw: &str) -> Result<Vec<u8>> {
    let path = resolve_media_ref(raw)?;
    if path.is_dir() {
        anyhow::bail!(
            "media ref is a directory; use media_understand mode=image with imageStart/imageEnd"
        );
    }
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
