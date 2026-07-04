//! Session sandbox directories under `{app_data}/session-sandboxes/`.
//!
//! Default workspace (when the user did not pick a project folder):
//! - `{session-sandboxes}/{session_user_id}/` when `session_user_id` is non-empty
//! - `{session-sandboxes}/_anonymous/{conversation_id}/` when it is empty
//!
//! Legacy `{session-sandboxes}/{conversation_id}/` paths are removed on conversation
//! delete only — not reused for new runs.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const SANDBOXES_DIR: &str = "session-sandboxes";
pub const ANONYMOUS_SEGMENT: &str = "_anonymous";

pub struct SessionSandbox;

impl SessionSandbox {
    fn sandboxes_base() -> Result<PathBuf> {
        Ok(crate::storage::app_data_dir()?.join(SANDBOXES_DIR))
    }

    /// Legacy per-conversation path — cleanup only; do not use for new runs.
    pub fn legacy_conversation_path(conversation_id: &str) -> Result<PathBuf> {
        let base = Self::sandboxes_base()?;
        let cid = crate::storage::sanitize_storage_dir_segment(conversation_id);
        Ok(base.join(cid))
    }

    /// Default sandbox path for a run (may not exist yet).
    pub fn default_path(conversation_id: &str, session_user_id: &str) -> Result<PathBuf> {
        let base = Self::sandboxes_base()?;
        let uid = session_user_id.trim();
        if uid.is_empty() {
            let cid = crate::storage::sanitize_storage_dir_segment(conversation_id);
            Ok(base.join(ANONYMOUS_SEGMENT).join(cid))
        } else {
            Ok(base.join(crate::storage::sanitize_storage_dir_segment(uid)))
        }
    }

    /// Create the default sandbox directory when needed.
    pub fn ensure_default(conversation_id: &str, session_user_id: &str) -> Result<PathBuf> {
        let path = Self::default_path(conversation_id, session_user_id)?;
        if !path.exists() {
            std::fs::create_dir_all(&path).with_context(|| {
                format!("Failed to create session sandbox: {}", path.display())
            })?;
        }
        Ok(path)
    }

    /// Whether `path` is the default sandbox for this conversation / user (may not exist).
    pub fn is_default_sandbox_path(
        conversation_id: &str,
        session_user_id: &str,
        path: &Path,
    ) -> Result<bool> {
        let expected = Self::default_path(conversation_id, session_user_id)?;
        let legacy = Self::legacy_conversation_path(conversation_id)?;
        Ok(path == expected || path == legacy)
    }

    /// Whether `path` is under `{app_data}/session-sandboxes/`.
    pub fn is_sandbox(path: &Path) -> Result<bool> {
        let base = Self::sandboxes_base()?.canonicalize();
        match base {
            Ok(base) => {
                let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
                Ok(path.starts_with(&base))
            }
            Err(_) => Ok(false),
        }
    }

    /// Remove per-conversation dirs on delete. Does **not** remove a shared user sandbox.
    pub fn cleanup_for_conversation(conversation_id: &str) -> Result<()> {
        for path in [
            Self::legacy_conversation_path(conversation_id)?,
            Self::default_path(conversation_id, "")?,
        ] {
            if path.exists() {
                std::fs::remove_dir_all(&path).with_context(|| {
                    format!("Failed to remove session sandbox: {}", path.display())
                })?;
            }
        }
        Ok(())
    }

    // --- Deprecated aliases (tests / gradual migration) ---

    #[deprecated(note = "use default_path or legacy_conversation_path")]
    pub fn path(conversation_id: &str) -> Result<PathBuf> {
        Self::legacy_conversation_path(conversation_id)
    }

    #[deprecated(note = "use ensure_default")]
    pub fn ensure(conversation_id: &str) -> Result<PathBuf> {
        Self::ensure_default(conversation_id, "")
    }

    #[deprecated(note = "use is_default_sandbox_path or is_sandbox")]
    pub fn is_path_for(conversation_id: &str, path: &Path) -> Result<bool> {
        Self::is_default_sandbox_path(conversation_id, "", path)
            .or_else(|_| Ok(path == Self::legacy_conversation_path(conversation_id)?))
    }

    #[deprecated(note = "use cleanup_for_conversation")]
    pub fn cleanup(conversation_id: &str) -> Result<()> {
        Self::cleanup_for_conversation(conversation_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_path_uses_session_user_id() {
        let path = SessionSandbox::default_path("conv_a", "user-42").unwrap();
        let s = path.display().to_string();
        assert!(s.contains("session-sandboxes"));
        assert!(s.contains("user-42"));
        assert!(!s.contains("conv_a"));
    }

    #[test]
    fn default_path_anonymous_uses_conversation_id() {
        let path = SessionSandbox::default_path("conv_a", "").unwrap();
        let s = path.display().to_string();
        assert!(s.contains("_anonymous"));
        assert!(s.contains("conv_a"));
    }

    #[test]
    fn sanitize_im_session_user_id_is_flat() {
        let id = "wecom:group:chat1";
        let safe = crate::storage::sanitize_storage_dir_segment(id);
        assert!(!safe.contains(':'));
        let path = SessionSandbox::default_path("conv_x", id).unwrap();
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some(safe.as_str())
        );
    }

    #[test]
    fn is_sandbox_returns_false_for_random_path() {
        let path = Path::new("/tmp");
        let result = SessionSandbox::is_sandbox(path).unwrap_or(false);
        assert!(!result);
    }
}
