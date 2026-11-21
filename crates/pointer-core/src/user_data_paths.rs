//! Host paths for the user-level `/data` layout.
//!
//! Guest paths stay under `/data/...`. These helpers only resolve the host
//! backing directories used by the media store and migration tools.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::env::mounts::{ensure_data_layout, volume_names_for};
use crate::user_key::home_volume_name;

/// `{app_data}/session-sandboxes/{user_key}-data` (or anon conversation volumes).
pub fn user_data_host_root(session_user_id: &str, conversation_id: &str) -> Result<PathBuf> {
    let base = crate::session_sandbox::SessionSandbox::sandboxes_base()?;
    let (_home, data) = volume_names_for(session_user_id, conversation_id);
    let root = base.join(data);
    ensure_data_layout(&root);
    Ok(root)
}

/// Host path for chat attachments under the user data volume.
pub fn attachments_host_dir(session_user_id: &str, conversation_id: &str) -> Result<PathBuf> {
    let root = user_data_host_root(session_user_id, conversation_id)?;
    let conv = crate::storage::sanitize_storage_dir_segment(conversation_id.trim());
    let dir = root.join("attachments").join(conv);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create attachments dir {}", dir.display()))?;
    Ok(dir)
}

/// Host path for chat media under the user data volume.
pub fn media_chat_host_dir(session_user_id: &str, conversation_id: &str) -> Result<PathBuf> {
    let root = user_data_host_root(session_user_id, conversation_id)?;
    let conv = crate::storage::sanitize_storage_dir_segment(conversation_id.trim());
    let dir = root.join("media").join("chat").join(conv);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create media chat dir {}", dir.display()))?;
    Ok(dir)
}

/// Host path for generated media under the user data volume.
pub fn media_generated_host_dir(session_user_id: &str, conversation_id: &str) -> Result<PathBuf> {
    let root = user_data_host_root(session_user_id, conversation_id)?;
    let conv = crate::storage::sanitize_storage_dir_segment(conversation_id.trim());
    let dir = root.join("media").join("generated").join(conv);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create generated media dir {}", dir.display()))?;
    Ok(dir)
}

/// Host path for computer captures under the user data volume.
pub fn captures_host_dir(
    session_user_id: &str,
    conversation_id: &str,
    date: &str,
) -> Result<PathBuf> {
    let root = user_data_host_root(session_user_id, conversation_id)?;
    let conv = crate::storage::sanitize_storage_dir_segment(conversation_id.trim());
    let dir = root.join("captures").join(date).join(conv);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create captures dir {}", dir.display()))?;
    Ok(dir)
}

/// Host home volume for a user (`{user_key}-home`).
pub fn user_home_host_root(session_user_id: &str) -> Result<PathBuf> {
    let base = crate::session_sandbox::SessionSandbox::sandboxes_base()?;
    Ok(base.join(home_volume_name(session_user_id)))
}

/// Writable plugin runtime data: `{home}/.pointer/plugin-data/{plugin_id}`.
pub fn plugin_data_host_dir(session_user_id: &str, plugin_id: &str) -> Result<PathBuf> {
    let home = user_home_host_root(session_user_id)?;
    let id = crate::storage::sanitize_storage_dir_segment(plugin_id.trim());
    let dir = home.join(".pointer").join("plugin-data").join(id);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create plugin-data dir {}", dir.display()))?;
    Ok(dir)
}

/// Plugin authorization store outside the plugin code tree.
pub fn plugin_auth_store_path(session_user_id: &str) -> Result<PathBuf> {
    let home = user_home_host_root(session_user_id)?;
    let dir = home.join(".pointer");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir.join("plugin-auth.json"))
}

/// Guest path for project workspaces.
pub fn guest_project_path(project_name: &str) -> String {
    let name = crate::storage::sanitize_storage_dir_segment(project_name.trim());
    format!("/data/workspaces/{name}")
}

/// Guest path for a managed worktree.
pub fn guest_worktree_path(conversation_id: &str) -> String {
    let key = crate::storage::sanitize_storage_dir_segment(conversation_id.trim());
    format!("/data/worktrees/{key}")
}

/// True when `host` is under a user data volume root.
pub fn is_under_user_data_host(host: &Path, session_user_id: &str, conversation_id: &str) -> bool {
    match user_data_host_root(session_user_id, conversation_id) {
        Ok(root) => host.starts_with(root),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_paths_are_stable() {
        assert_eq!(guest_project_path("Demo App"), "/data/workspaces/Demo_App");
        assert!(!guest_worktree_path("a:b").contains(':'));
    }

    #[test]
    fn media_dirs_are_created_under_data_volume() {
        let _guard = crate::storage::test_app_data_dir_lock();
        let temp = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(temp.path().to_path_buf());
        let dir = media_chat_host_dir("alice", "conv-1").unwrap();
        assert!(dir.is_dir());
        assert!(dir.to_string_lossy().contains("-data"));
        assert!(dir.to_string_lossy().contains("media/chat"));
    }
}
