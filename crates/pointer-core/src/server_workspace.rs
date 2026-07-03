//! Per-user default workspace roots for pointer-server (web mode).

use anyhow::{Context, Result};

/// True when running pointer-server web mode (platform auth is not persisted to auth.dat).
pub fn is_pointer_server_mode() -> bool {
    !crate::storage::platform_auth_persist_enabled()
}

/// Sanitize platform user id for use as a single directory name under app data.
pub fn sanitize_login_user_segment(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "anonymous".to_string();
    }
    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        match ch {
            '/' | '\\' | ':' | '\0' => out.push('_'),
            c if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' => out.push(c),
            _ => out.push('_'),
        }
    }
    if out.is_empty() {
        "anonymous".to_string()
    } else {
        out
    }
}

/// `{app_data_dir}/{login_user}` — created on first use.
pub fn ensure_server_user_workspace(login_user_id: &str) -> Result<String> {
    let app_dir = crate::storage::app_data_dir().context("resolve app data dir")?;
    let segment = sanitize_login_user_segment(login_user_id);
    let path = app_dir.join(&segment);
    if !path.is_dir() {
        std::fs::create_dir_all(&path).with_context(|| {
            format!("create server user workspace {}", path.display())
        })?;
        log::info!(
            "server_workspace: created user workspace app_dir={} login_user={} path={}",
            app_dir.display(),
            segment,
            path.display()
        );
    }
    path.canonicalize()
        .map(|p| p.display().to_string())
        .or_else(|_| Ok(path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_login_user_replaces_unsafe_chars() {
        assert_eq!(sanitize_login_user_segment("user:123/abc"), "user_123_abc");
        assert_eq!(sanitize_login_user_segment(""), "anonymous");
    }
}
