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
    // 已绑定远程环境时，工作区是**沙箱路径**（`/workspace`）：settings / 会话存储
    // 里记的是宿主路径，在沙箱里根本不存在。
    //
    // 必须看 `current_env()`（本线程实际绑定的环境），不能看 `effective_env()`：
    // 后者在未绑定时会按**部署默认档位**返回 DeniedEnv，把 local 账号
    // （`[auth] admins` 里的账号）也当成远程，工作区被错误地改成 `/workspace`。
    if let Some(env) = crate::env::current_env() {
        if env.kind() != crate::env::EnvKind::Local {
            return env.workspace_root().display().to_string();
        }
    }
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

    /// 回归：部署默认档位是 desktop、但本线程没有绑定环境（local 账号）时，
    /// 工作区必须用宿主路径，不能因为 `effective_env()` 返回 DeniedEnv 而变成 `/workspace`。
    #[test]
    fn unbound_thread_keeps_host_workspace_even_when_deployment_is_remote() {
        // 把部署默认设成 desktop；若实现里还读 `effective_env()`，这里就会返回
        // `/workspace`，测试失败。
        std::env::set_var("POINTER_SERVER_ENVIRONMENT_PROVIDER", "desktop");
        let host = std::env::temp_dir().join("pointer-ws-regression");
        let got = resolve_tool_workspace("conv-x", Some("user-x"), &host.display().to_string(), "test");
        assert_eq!(
            got,
            host.display().to_string(),
            "未绑定环境时必须用宿主工作区"
        );
        assert_ne!(got, "/workspace");
        std::env::remove_var("POINTER_SERVER_ENVIRONMENT_PROVIDER");
    }

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
