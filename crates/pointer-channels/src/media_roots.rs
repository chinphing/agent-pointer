use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
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
