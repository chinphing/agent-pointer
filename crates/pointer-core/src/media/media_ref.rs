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

    match resolve_local_media_path(rel) {
        Ok(path) => Ok(path),
        Err(e) => {
            log::warn!("resolve_media_ref FAILED: {trimmed} error: {e:#}");
            Err(e).with_context(|| format!("resolve media ref {trimmed}"))
        }
    }
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
            "media ref is a directory; use media_understand mode=image with refs containing one directory path and pageStart/pageEnd"
        );
    }
    let session = crate::env::require_session()?;
    let host = session_host_file(session.environment().as_ref(), &path)?;
    std::fs::read(&host).with_context(|| format!("read media file {}", host.display()))
}

/// 用户会话里只认路径表。`to_host` 成功就用返回的宿主路径；
/// `to_agent` 成功说明入参已经是宿主路径。都不中返回 `PATH_NOT_MAPPED`。
pub(crate) fn session_host_file(
    env: &dyn crate::env::Environment,
    path: &std::path::Path,
) -> Result<std::path::PathBuf> {
    if env.is_denied() || env.path_map().is_host_passthrough() {
        log::warn!(
            "media: refuse passthrough environment for {}",
            path.display()
        );
        anyhow::bail!("缺少当前用户的执行环境");
    }
    if let Ok(host) = env.path_map().to_host(path) {
        return Ok(host);
    }
    if env.path_map().to_agent(path).is_ok() {
        return Ok(path.to_path_buf());
    }
    log::warn!(
        "media: path is outside the session home and data: {}",
        path.display()
    );
    anyhow::bail!(
        "PATH_NOT_MAPPED: 媒体路径不在当前用户的 home 或 data 内: {}",
        path.display()
    )
}

/// 宿主绝对路径必须落在当前会话里。没有会话就拒绝，不再按整机路径打开。
pub(crate) fn ensure_inside_user_env(path: &std::path::Path) -> Result<()> {
    let session = crate::env::require_session()?;
    session_host_file(session.environment().as_ref(), path).map(|_| ())
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
