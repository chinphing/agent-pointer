use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use pointer_core::media::path_hint::MEDIA_URI_SCHEME;

/// Strip a `file://` URI to a local path string (`file:///Users/a` → `/Users/a`).
pub fn strip_file_uri(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if !trimmed.to_lowercase().starts_with("file://") {
        return None;
    }
    let rest = trimmed
        .get(7..)
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    // file:///C:/path on Windows → C:/path
    if rest.starts_with('/') {
        let without = rest.trim_start_matches('/');
        if without.len() >= 2 {
            let bytes = without.as_bytes();
            if bytes.get(1) == Some(&b':') && bytes.first().is_some_and(|c| c.is_ascii_alphabetic()) {
                return Some(without.replace('\\', "/"));
            }
        }
        return Some(rest.to_string());
    }
    Some(rest.replace('\\', "/"))
}

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
    if strip_file_uri(trimmed).is_some() {
        return true;
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
    if let Some(path) = strip_file_uri(trimmed) {
        return Ok(PathBuf::from(path));
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

    #[test]
    fn strip_file_uri_unix_absolute() {
        let uri = "file:///Users/me/project/index.html";
        assert_eq!(
            strip_file_uri(uri).as_deref(),
            Some("/Users/me/project/index.html")
        );
        assert!(is_user_filesystem_path(uri));
    }

    #[test]
    fn strip_file_uri_windows_drive() {
        let uri = "file:///C:/Users/me/report.pdf";
        assert_eq!(
            strip_file_uri(uri).as_deref(),
            Some("C:/Users/me/report.pdf")
        );
    }
}
