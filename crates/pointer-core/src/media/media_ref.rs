//! Resolve media references (`pointer-media://`, storage rel, absolute paths).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::path_hint::MEDIA_URI_SCHEME;
use super::resolve::resolve_local_media_path;

/// Resolve a media reference to an environment path (guest under sandbox, host under local user).
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

/// True when `raw` resolves to an existing directory in the session environment.
pub fn media_ref_is_directory(raw: &str) -> Result<bool> {
    let path = resolve_media_ref(raw)?;
    let session = crate::env::require_session()?;
    let env = session.environment();
    let path = session_env_path(env.as_ref(), &path)?;
    Ok(env.metadata(&path).map(|m| m.is_dir).unwrap_or(false))
}

/// Read raw bytes for a resolved media reference (files only).
pub fn read_media_ref_bytes(raw: &str) -> Result<Vec<u8>> {
    let path = resolve_media_ref(raw)?;
    let session = crate::env::require_session()?;
    let env = session.environment();
    let path = session_env_path(env.as_ref(), &path)?;
    let meta = env
        .metadata(&path)
        .with_context(|| format!("read media metadata {}", path.display()))?;
    if meta.is_dir {
        anyhow::bail!(
            "media ref is a directory; use media_understand mode=image with refs containing one directory path and pageStart/pageEnd"
        );
    }
    env.read_file(&path)
        .with_context(|| format!("read media file {}", path.display()))
}

/// 会话内可读路径：优先 guest；若入参已是宿主路径且能翻回 guest，则返回 guest。
///
/// 不再返回宿主路径供 `std::fs` 使用——调用方应经 [`Environment`] 读写。
pub(crate) fn session_env_path(env: &dyn crate::env::Environment, path: &Path) -> Result<PathBuf> {
    if env.is_denied() || env.path_map().is_host_passthrough() {
        log::warn!(
            "media: refuse passthrough environment for {}",
            path.display()
        );
        anyhow::bail!("缺少当前用户的执行环境");
    }
    if env.path_map().contains_guest(path) {
        return Ok(path.to_path_buf());
    }
    // LocalEnv::for_user 的 guest==host；入参可能是宿主绝对路径。
    if let Ok(guest) = env.path_map().to_agent(path) {
        return Ok(guest);
    }
    // 挂载表未 canonicalize、入参已 realpath（或相反）时再试一次。
    if let Ok(canon) = path.canonicalize() {
        if canon != path {
            if let Ok(guest) = env.path_map().to_agent(&canon) {
                return Ok(guest);
            }
        }
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

/// 兼容旧名：校验路径在会话内，返回环境路径（非宿主旁路）。
pub(crate) fn session_host_file(env: &dyn crate::env::Environment, path: &Path) -> Result<PathBuf> {
    session_env_path(env, path)
}

/// 路径必须落在当前会话里。没有会话就拒绝。
pub(crate) fn ensure_inside_user_env(path: &Path) -> Result<()> {
    let session = crate::env::require_session()?;
    session_env_path(session.environment().as_ref(), path).map(|_| ())
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
