//! Path allowlist for App UI media preview (local files referenced in assistant replies).

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use dirs::home_dir;

use super::path_hint::MEDIA_URI_SCHEME;
use super::store::conversation_media_root;

pub fn expand_root(raw: &str) -> Result<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty media root");
    }
    if trimmed == "~" {
        let home = home_dir().context("home dir for ~")?;
        return Ok(home);
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        let home = home_dir().context("home dir for ~/")?;
        return Ok(home.join(rest));
    }
    if let Some(rest) = trimmed.strip_prefix("~\\") {
        let home = home_dir().context("home dir for ~\\")?;
        return Ok(home.join(rest));
    }
    Ok(PathBuf::from(trimmed))
}

pub fn configured_roots(roots: &[String]) -> Vec<PathBuf> {
    roots
        .iter()
        .filter_map(|r| expand_root(r).ok())
        .collect()
}

pub fn is_under_conversation_media(path: &Path) -> bool {
    let Ok(root) = conversation_media_root() else {
        return false;
    };
    let Ok(canonical_root) = root.canonicalize() else {
        return path.starts_with(&root);
    };
    let Ok(canonical) = path.canonicalize() else {
        return path.starts_with(&canonical_root);
    };
    canonical.starts_with(&canonical_root)
}

pub fn path_has_traversal(raw: &str) -> bool {
    Path::new(raw.trim())
        .components()
        .any(|c| matches!(c, Component::ParentDir))
}

/// True when `raw` refers to a user machine path (not `conversation-media/` rel).
pub fn is_user_filesystem_path(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed == "~" || trimmed.starts_with("~/") || trimmed.starts_with("~\\") {
        return true;
    }
    Path::new(trimmed).is_absolute()
}

pub fn normalize_user_path(raw: &str) -> Result<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty path");
    }
    if trimmed.starts_with(MEDIA_URI_SCHEME) {
        return Ok(PathBuf::from(trimmed));
    }
    if trimmed == "~" || trimmed.starts_with("~/") || trimmed.starts_with("~\\") {
        return expand_root(trimmed);
    }
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        return Ok(path);
    }
    if trimmed.starts_with("./") || trimmed.starts_with("../") || trimmed.contains('/') {
        return Ok(std::env::current_dir()?.join(trimmed));
    }
    Ok(path)
}

pub fn assert_app_media_preview_allowed(path: &Path, extra_roots: &[String]) -> Result<()> {
    if is_under_conversation_media(path) {
        return Ok(());
    }

    let canonical = path
        .canonicalize()
        .with_context(|| format!("resolve media path {}", path.display()))?;

    if let Some(home) = home_dir() {
        let home_canon = home.canonicalize().unwrap_or(home);
        if canonical.starts_with(&home_canon) {
            return Ok(());
        }
    }

    for root in configured_roots(extra_roots) {
        let root_canon = root.canonicalize().unwrap_or(root);
        if canonical.starts_with(&root_canon) {
            return Ok(());
        }
    }

    anyhow::bail!(
        "media preview path not allowed: {} (add parent dir to IM 出站媒体路径 or use conversation-media)",
        canonical.display()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilde_desktop_is_user_filesystem_path() {
        assert!(is_user_filesystem_path("~/Desktop/baby_cover.jpg"));
        assert!(!is_user_filesystem_path("conv-id/att.png"));
    }

    #[test]
    fn normalize_expands_tilde() {
        let home = home_dir().expect("home");
        let path = normalize_user_path("~/Desktop/baby_cover.jpg").expect("path");
        assert_eq!(path, home.join("Desktop/baby_cover.jpg"));
    }
}
