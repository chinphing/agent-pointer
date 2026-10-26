//! Where a tool runs: one classification + one fallback chain.
//!
//! Before this module, three call sites repeated the same "settings → persisted
//! conversation workspace → session sandbox" chain (`registry`, `dispatch::terminal`,
//! `workspace_delegation`). Adding a new workspace flavour (a managed git worktree)
//! meant editing all of them and hoping they stayed aligned. The chain now lives here.
//!
//! `classify_workspace` is the read-only label used by the UI and the API; it never
//! changes behaviour and is not on the tool hot path.

use std::path::Path;

/// What kind of directory a conversation's workspace is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceKind {
    /// A user-picked project folder (may be any directory).
    Project,
    /// The per-user / per-conversation scratch sandbox under app data.
    Sandbox,
    /// A managed git worktree created by this app.
    Worktree,
}

impl WorkspaceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Sandbox => "sandbox",
            Self::Worktree => "worktree",
        }
    }

    /// Sandboxes are disposable scratch space; projects and worktrees are not.
    pub fn is_ephemeral(self) -> bool {
        matches!(self, Self::Sandbox)
    }
}

/// Label a workspace path. Unknown / missing paths are reported as [`WorkspaceKind::Project`].
pub fn classify_workspace(path: &Path) -> WorkspaceKind {
    if crate::session_sandbox::SessionSandbox::is_sandbox(path).unwrap_or(false) {
        return WorkspaceKind::Sandbox;
    }
    let managed = crate::worktree::WorktreeManager::for_app()
        .map(|manager| manager.is_managed_path(path))
        .unwrap_or(false);
    if managed {
        return WorkspaceKind::Worktree;
    }
    WorkspaceKind::Project
}

/// Resolve the directory a file/terminal tool must use.
///
/// Order (identical for every tool):
/// 1. non-empty `from_settings` (already resolved at run start);
/// 2. the conversation's persisted workspace;
/// 3. the session sandbox, created on demand.
///
/// `source` is only a log prefix (`registry`, `terminal`, …).
pub fn resolve_tool_workspace(
    conversation_id: &str,
    session_user_id: Option<&str>,
    from_settings: &str,
    source: &str,
) -> String {
    let trimmed = from_settings.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    if let Ok(store) = crate::conversation_store::global_store() {
        if let Ok(workspace) = store.workspace_root(conversation_id) {
            if !workspace.trim().is_empty() {
                log::info!(
                    "{source}: workspace from conversation store conversation_id={conversation_id}: {workspace}"
                );
                return workspace.trim().to_string();
            }
        }
    }
    let owned_user_id;
    let user_id = match session_user_id {
        Some(id) => id,
        None => {
            owned_user_id = crate::conversation_store::global_store()
                .ok()
                .and_then(|store| store.session_user_id(conversation_id).ok())
                .unwrap_or_default();
            owned_user_id.as_str()
        }
    };
    match crate::session_sandbox::SessionSandbox::ensure_default(conversation_id, user_id) {
        Ok(path) => {
            let workspace = path.display().to_string();
            log::info!(
                "{source}: workspace from session sandbox conversation_id={conversation_id}: {workspace}"
            );
            workspace
        }
        Err(error) => {
            log::warn!(
                "{source}: failed to ensure session sandbox conversation_id={conversation_id}: {error:#}"
            );
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn kinds_render_stable_labels() {
        assert_eq!(WorkspaceKind::Project.as_str(), "project");
        assert_eq!(WorkspaceKind::Sandbox.as_str(), "sandbox");
        assert_eq!(WorkspaceKind::Worktree.as_str(), "worktree");
        assert!(WorkspaceKind::Sandbox.is_ephemeral());
        assert!(!WorkspaceKind::Worktree.is_ephemeral());
        assert!(!WorkspaceKind::Project.is_ephemeral());
    }

    #[test]
    fn sandbox_paths_classify_as_sandbox() {
        let _guard = crate::storage::test_app_data_dir_lock();
        let temp = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(temp.path().to_path_buf());
        let sandbox = temp.path().join("session-sandboxes").join("user-1");
        std::fs::create_dir_all(&sandbox).unwrap();
        assert_eq!(classify_workspace(&sandbox), WorkspaceKind::Sandbox);
        assert_eq!(
            classify_workspace(Path::new("/tmp/not-a-sandbox")),
            WorkspaceKind::Project
        );
    }

    #[test]
    fn empty_settings_fall_back_to_the_sandbox() {
        let _guard = crate::storage::test_app_data_dir_lock();
        let temp = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(temp.path().to_path_buf());
        let resolved = resolve_tool_workspace("conv-test", Some("user-1"), "  ", "test");
        assert!(!resolved.is_empty());
        assert!(PathBuf::from(&resolved).is_dir());
        assert!(resolved.contains("session-sandboxes"));
    }
}
