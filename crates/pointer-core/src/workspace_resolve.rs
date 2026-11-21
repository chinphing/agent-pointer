//! Where a tool runs: one classification + one fallback chain.
//!
//! Before this module, three call sites repeated the same "settings → persisted
//! conversation workspace → session sandbox" chain (`registry`, `dispatch::terminal`,
//! `workspace_delegation`). Adding a new workspace flavour (a managed git worktree)
//! meant editing all of them and hoping they stayed aligned. The chain now lives here.
//!
//! `classify_workspace` is the read-only label used by the UI and the API; it never
//! changes behaviour and is not on the tool hot path.


use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// Absolute workspace path inside the current Environment namespace.
///
/// This is intentionally not a persisted kind. Local paths look like
/// `/Users/a/project`; sandbox project paths look like `/data/workspaces/a`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSelection {
    pub path: String,
}

/// What the current account may browse / create / select as a workspace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePolicy {
    /// Absolute roots that may appear in the directory chooser.
    pub selectable_roots: Vec<String>,
    pub can_create_directories: bool,
    /// Reserved relative names under a selectable root that must not be chosen.
    pub reserved_names: Vec<String>,
}

/// Resolved roots for one Project/Conversation execution context.
///
/// This is conversation-scoped state, not a mutable field on Environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedWorkspace {
    pub owner_user_key: String,
    /// Persistent user data root (`/data` in sandbox, unused/empty for pure local).
    pub data_root: Option<PathBuf>,
    /// Active project or worktree root in the Environment namespace.
    pub workspace_root: PathBuf,
    /// Default cwd for exec/spawn/PTY.
    pub active_cwd: PathBuf,
}

impl ResolvedWorkspace {
    pub fn active_root(&self) -> &Path {
        &self.active_cwd
    }
}

/// Guest path of the selectable projects tree in sandbox Environments.
pub const SANDBOX_WORKSPACES_ROOT: &str = "/data/workspaces";
/// Guest path of the persistent data root in sandbox Environments.
pub const SANDBOX_DATA_ROOT: &str = "/data";

/// True when `path` is an absolute Unix guest path under `/data`.
pub fn looks_like_sandbox_guest_path(path: &str) -> bool {
    let trimmed = path.trim().replace('\\', "/");
    trimmed == "/data" || trimmed.starts_with("/data/")
}

/// True when `path` looks like an absolute host path that must not enter sandbox APIs.
pub fn looks_like_host_absolute_path(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }
    if looks_like_sandbox_guest_path(trimmed) {
        return false;
    }
    let p = Path::new(trimmed);
    if p.is_absolute() {
        return true;
    }
    // Windows drive / UNC written with forward or back slashes.
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() >= 3
        && chars[0].is_ascii_alphabetic()
        && chars[1] == ':'
        && (chars[2] == '/' || chars[2] == '\\')
    {
        return true;
    }
    trimmed.starts_with("\\\\") || trimmed.starts_with("//")
}

/// Validate that `candidate` stays under `root` without `..` escape.
///
/// This is a pure lexical check used before Environment-aware realpath. Existing
/// files still need Environment-side canonicalization in later phases.
pub fn ensure_descendant_path(root: &Path, candidate: &Path) -> Result<PathBuf> {
    if root.as_os_str().is_empty() {
        bail!("工作区可选根不能为空");
    }
    if candidate.as_os_str().is_empty() {
        bail!("路径不能为空");
    }
    if candidate
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        bail!("路径不能包含 ..");
    }
    let root_norm = normalize_lexical(root);
    let cand_norm = if candidate.is_absolute() {
        normalize_lexical(candidate)
    } else {
        let mut joined = root_norm.clone();
        joined.push(candidate);
        normalize_lexical(&joined)
    };
    if !path_starts_with(&cand_norm, &root_norm) {
        bail!("路径越出可选根");
    }
    Ok(cand_norm)
}

/// Sandbox chooser policy: only `/data/workspaces` and its descendants.
pub fn sandbox_workspace_policy() -> WorkspacePolicy {
    WorkspacePolicy {
        selectable_roots: vec![SANDBOX_WORKSPACES_ROOT.to_string()],
        can_create_directories: true,
        reserved_names: vec![
            "attachments".into(),
            "media".into(),
            "captures".into(),
            "worktrees".into(),
        ],
    }
}

/// Reject sandbox selection payloads that smuggle host absolute paths.
pub fn reject_sandbox_host_path(path: &str) -> Result<()> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        bail!("工作区路径不能为空");
    }
    if looks_like_host_absolute_path(trimmed) {
        log::warn!("workspace: rejected host absolute path in sandbox selection");
        bail!("沙箱工作区只能选择 /data/workspaces 下的目录");
    }
    if trimmed == SANDBOX_WORKSPACES_ROOT || trimmed.starts_with(&format!("{SANDBOX_WORKSPACES_ROOT}/")) {
        return Ok(());
    }
    // Relative names are resolved under the selectable root.
    ensure_descendant_path(Path::new(SANDBOX_WORKSPACES_ROOT), Path::new(trimmed))
        .with_context(|| format!("非法沙箱工作区路径: {trimmed}"))?;
    Ok(())
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                let _ = out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn path_starts_with(path: &Path, root: &Path) -> bool {
    let path_comps: Vec<_> = path.components().collect();
    let root_comps: Vec<_> = root.components().collect();
    path_comps.starts_with(&root_comps)
}


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
    // 已绑定远程环境时，工作区是**沙箱路径**（`/data/workspaces/...`）：settings / 会话存储
    // 里记的是宿主路径，在沙箱里根本不存在。
    //
    // 必须看 `current_env()`（本线程实际绑定的环境），不能看 `effective_env()`：
    // 后者在未绑定时会按**部署默认档位**返回 DeniedEnv，把 local 账号
    // （`[auth] admins` 里的账号）也当成远程，工作区被错误地改成沙箱路径。
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
        assert_ne!(got, "/data/workspaces");
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

    #[test]
    fn sandbox_policy_rejects_host_paths() {
        assert!(reject_sandbox_host_path("/Users/alice/project").is_err());
        assert!(reject_sandbox_host_path("C:/Users/alice/project").is_err());
        assert!(reject_sandbox_host_path("/data/../etc/passwd").is_err());
        assert!(reject_sandbox_host_path("/data/workspaces/demo").is_ok());
        assert!(reject_sandbox_host_path("demo").is_ok());
    }

    #[test]
    fn descendant_check_blocks_parent_escape() {
        let root = Path::new("/data/workspaces");
        assert!(ensure_descendant_path(root, Path::new("a/b")).is_ok());
        assert!(ensure_descendant_path(root, Path::new("../workspaces-x")).is_err());
        assert!(ensure_descendant_path(root, Path::new("/data/workspaces/../etc")).is_err());
    }

}
