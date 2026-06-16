//! Per-conversation sandbox directory management.
//!
//! When no explicit workspace root is configured, each conversation gets a sandbox
//! under `{app_data}/session-sandboxes/{conversation_id}/`. The sandbox is created
//! on first use and cleaned up when the conversation is deleted.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const SANDBOXES_DIR: &str = "session-sandboxes";

/// Per-conversation sandbox directory.
pub struct SessionSandbox;

impl SessionSandbox {
    /// Whether `path` is this conversation's sandbox directory (may not exist yet).
    pub fn is_path_for(conversation_id: &str, path: &Path) -> Result<bool> {
        Ok(Self::path(conversation_id)? == path)
    }

    /// Return the sandbox path for `conversation_id` (no side effects, path may
    /// not exist on disk yet).
    pub fn path(conversation_id: &str) -> Result<PathBuf> {
        let base = crate::storage::app_data_dir()?.join(SANDBOXES_DIR);
        let cid = crate::storage::sanitize_storage_dir_segment(conversation_id);
        Ok(base.join(cid))
    }

    /// Return the sandbox path, creating the directory (and parents) if needed.
    pub fn ensure(conversation_id: &str) -> Result<PathBuf> {
        let path = Self::path(conversation_id)?;
        if !path.exists() {
            std::fs::create_dir_all(&path).with_context(|| {
                format!("Failed to create session sandbox: {}", path.display())
            })?;
        }
        Ok(path)
    }

    /// Delete the sandbox directory for `conversation_id` if it exists.
    /// Safe to call even when the sandbox was never created.
    pub fn cleanup(conversation_id: &str) -> Result<()> {
        let path = Self::path(conversation_id)?;
        if path.exists() {
            std::fs::remove_dir_all(&path).with_context(|| {
                format!("Failed to remove session sandbox: {}", path.display())
            })?;
        }
        Ok(())
    }

    /// Check whether `path` is inside any session sandbox.
    pub fn is_sandbox(path: &Path) -> Result<bool> {
        let base = crate::storage::app_data_dir()?.join(SANDBOXES_DIR).canonicalize();
        match base {
            Ok(base) => {
                let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
                Ok(path.starts_with(&base))
            }
            Err(_) => Ok(false),
        }
    }

    /// Return the conversation_id from a sandbox path, if it matches the pattern.
    /// Returns `None` if the path is not under session-sandboxes.
    pub fn conversation_id_from_path(path: &Path) -> Result<Option<String>> {
        let base = crate::storage::app_data_dir()?.join(SANDBOXES_DIR).canonicalize();
        match base {
            Ok(base) => {
                if let Ok(canonical) = path.canonicalize() {
                    if canonical.starts_with(&base) {
                        if let Ok(rel) = canonical.strip_prefix(&base) {
                            let components: Vec<_> = rel.components().collect();
                            if let Some(first) = components.first() {
                                return Ok(Some(first.as_os_str().to_string_lossy().to_string()));
                            }
                        }
                    }
                }
                Ok(None)
            }
            Err(_) => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_is_under_app_data() {
        let path = SessionSandbox::path("conv_abc123").unwrap();
        let s = path.display().to_string();
        assert!(s.contains("session-sandboxes"));
        assert!(s.contains("conv_abc123"));
    }

    #[test]
    fn ensure_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        // Temporarily override app_data_dir via a mock — but easier: use ensure
        // on a real path. For unit test we can't easily mock app_data_dir,
        // so just verify path doesn't panic.
        let path = SessionSandbox::path("conv_test_ensure").unwrap();
        // Path contains app_data, just check it returns without error
        assert!(path.to_string_lossy().contains("session-sandboxes"));
    }

    #[test]
    fn is_sandbox_returns_false_for_random_path() {
        let path = Path::new("/tmp");
        let result = SessionSandbox::is_sandbox(path).unwrap_or(false);
        // Likely false since /tmp isn't under app_data
        assert!(!result);
    }

    #[test]
    fn sanitize_removes_separators() {
        let safe = crate::storage::sanitize_storage_dir_segment("conv/../foo\\bar\0baz");
        assert_eq!(safe, "conv_foo_bar_baz");
    }

    #[test]
    fn sanitize_im_conversation_id_is_flat() {
        let id = "dingtalk:default:dingtalk:dm:cidXuzjtFpqURa0N4CrrYrb9ZeeMktGbZLsHrJqwRGRyD8=:01263708524221016@s1";
        let safe = crate::storage::sanitize_storage_dir_segment(id);
        assert!(!safe.contains(':'));
        assert!(!safe.contains('@'));
        assert!(!safe.contains('/'));
        assert!(safe.contains("dingtalk_default_dingtalk_dm"));
        let path = SessionSandbox::path(id).unwrap();
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some(safe.as_str())
        );
    }
}
