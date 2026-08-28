//! Path allowlist for App UI media preview (local files referenced in assistant replies).

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use dirs::home_dir;

use super::path_hint::MEDIA_URI_SCHEME;
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

pub fn path_has_traversal(raw: &str) -> bool {
    Path::new(raw.trim())
        .components()
        .any(|c| matches!(c, Component::ParentDir))
}

/// Strip a `file://` URI to a local path string (`file:///Users/a` → `/Users/a`).
pub fn strip_file_uri(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if !trimmed.to_lowercase().starts_with("file://") {
        return None;
    }
    let rest = trimmed.get(7..).map(str::trim).filter(|s| !s.is_empty())?;
    if rest.starts_with('/') {
        let without = rest.trim_start_matches('/');
        if without.len() >= 2 {
            let bytes = without.as_bytes();
            if bytes.get(1) == Some(&b':') && bytes.first().is_some_and(|c| c.is_ascii_alphabetic())
            {
                return Some(without.replace('\\', "/"));
            }
        }
        return Some(rest.to_string());
    }
    Some(rest.replace('\\', "/"))
}

/// True when `raw` refers to a user machine path (not `conversation-media/` rel).
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
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && bytes.get(2).is_some_and(|c| *c == b'\\' || *c == b'/')
    {
        return true;
    }
    Path::new(trimmed).is_absolute()
}

/// Recover an absolute user path that was joined onto a workspace/sandbox root
/// after its leading `/` was stripped (e.g. `{sandbox}/Users/me/.pi/agent/models.json`).
pub fn recover_nested_absolute_path(path: &Path) -> Option<PathBuf> {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.len() < 2 {
        return None;
    }
    for marker in ["/Users/", "/home/", "/tmp/", "/private/", "/opt/", "/var/"] {
        let mut last_idx = None;
        let mut start = 0;
        while let Some(rel) = normalized.get(start..).and_then(|s| s.find(marker)) {
            let idx = start + rel;
            if idx > 0 {
                last_idx = Some(idx);
            }
            start = idx + marker.len();
            if start >= normalized.len() {
                break;
            }
        }
        if let Some(idx) = last_idx {
            let nested = PathBuf::from(&normalized[idx..]);
            if nested.is_file() || nested.is_dir() {
                return Some(nested);
            }
        }
    }
    recover_nested_windows_drive_path(&normalized)
}

fn recover_nested_windows_drive_path(normalized: &str) -> Option<PathBuf> {
    let bytes = normalized.as_bytes();
    let mut last_drive = None;
    let mut i = 1;
    while i + 1 < bytes.len() {
        if bytes[i] == b':'
            && bytes[i - 1].is_ascii_alphabetic()
            && (bytes[i + 1] == b'/' || bytes[i + 1] == b'\\')
            && i > 1
        {
            last_drive = Some(i - 1);
        }
        i += 1;
    }
    let idx = last_drive?;
    let nested = PathBuf::from(&normalized[idx..]);
    if nested.is_file() || nested.is_dir() {
        Some(nested)
    } else {
        None
    }
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
    if trimmed.starts_with("./") || trimmed.starts_with("../") {
        return Ok(std::env::current_dir()?.join(trimmed));
    }
    Ok(path)
}

pub fn assert_app_media_preview_allowed(path: &Path) -> Result<()> {
    if !path.is_file() {
        anyhow::bail!("media path is not a file: {}", path.display());
    }
    path.canonicalize()
        .with_context(|| format!("resolve media path {}", path.display()))?;
    Ok(())
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

    #[test]
    fn recover_nested_absolute_from_sandbox_join() {
        use std::fs;
        let real = std::env::temp_dir().join(format!(
            "pointer-nested-abs-{}.json",
            uuid::Uuid::new_v4()
        ));
        fs::write(&real, b"{}").unwrap();
        let real_unix = real.to_string_lossy().replace('\\', "/");
        let suffix = real_unix.trim_start_matches('/');
        let doubled = PathBuf::from(format!("/tmp/fake-session-sandbox/{suffix}"));
        let recovered = recover_nested_absolute_path(&doubled).expect("recover nested abs");
        assert_eq!(recovered, real);
        let _ = fs::remove_file(&real);
    }
}
