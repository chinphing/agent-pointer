use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use pointer_core::media::store::conversation_media_root;
use pointer_core::media::path_hint::MEDIA_URI_SCHEME;

pub fn expand_root(raw: &str) -> Result<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty media root");
    }
    if trimmed == "~" {
        let home = dirs::home_dir().context("home dir for ~")?;
        return Ok(home);
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        let home = dirs::home_dir().context("home dir for ~/")?;
        return Ok(home.join(rest));
    }
    if let Some(rest) = trimmed.strip_prefix("~\\") {
        let home = dirs::home_dir().context("home dir for ~\\")?;
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

pub fn is_pointer_media_uri(raw: &str) -> bool {
    raw.trim().starts_with(MEDIA_URI_SCHEME)
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

pub fn assert_media_path_allowed(path: &Path, extra_roots: &[String]) -> Result<()> {
    if is_under_conversation_media(path) {
        return Ok(());
    }

    let canonical = path
        .canonicalize()
        .with_context(|| format!("resolve media path {}", path.display()))?;

    for root in configured_roots(extra_roots) {
        let root_canon = root.canonicalize().unwrap_or(root);
        if canonical.starts_with(&root_canon) {
            return Ok(());
        }
    }

    anyhow::bail!(
        "outbound media path not allowed: {} (add parent dir to channels.meta.mediaLocalRoots)",
        canonical.display()
    );
}

pub fn normalize_user_path(raw: &str) -> Result<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty path");
    }
    if trimmed.starts_with(MEDIA_URI_SCHEME) {
        return Ok(PathBuf::from(trimmed));
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

pub fn path_has_traversal(raw: &str) -> bool {
    let path = Path::new(raw.trim());
    path.components().any(|c| matches!(c, Component::ParentDir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_traversal() {
        assert!(path_has_traversal("../secret.pdf"));
        assert!(!path_has_traversal("report.pdf"));
    }
}
