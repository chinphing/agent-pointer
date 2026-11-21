//! Shared workspace browse / validate / select service for Client and Web.
//!
//! Entry points (HTTP / Tauri) only authenticate; all policy and path decisions
//! live here so both runtimes stay aligned.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::env::provider::{ProviderConfig, Tier};
use crate::env::{Environment, LocalEnv};
use crate::user_key::user_key;
use crate::workspace_resolve::{
    ensure_descendant_path, looks_like_host_absolute_path, reject_sandbox_host_path,
    sandbox_workspace_policy, ResolvedWorkspace, WorkspacePolicy, WorkspaceSelection,
    SANDBOX_DATA_ROOT,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceListResult {
    pub parent: String,
    pub entries: Vec<WorkspaceDirEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSelectResult {
    pub path: String,
    pub display_path: String,
}

/// Resolve WorkspacePolicy for the authenticated account.
pub fn policy_for_user(session_user_id: &str) -> Result<WorkspacePolicy> {
    let cfg = ProviderConfig::from_env().context("read environment config")?;
    let uid = session_user_id.trim();
    let resolved = cfg.resolve(if uid.is_empty() { None } else { Some(uid) })?;
    Ok(match resolved.tier {
        Tier::Local => local_workspace_policy(),
        Tier::Headless | Tier::Desktop => sandbox_workspace_policy(),
    })
}

fn local_workspace_policy() -> WorkspacePolicy {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(home.display().to_string());
    }
    WorkspacePolicy {
        selectable_roots: roots,
        can_create_directories: true,
        reserved_names: Vec::new(),
    }
}

/// List selectable directories under `parent` according to the account policy.
pub fn list_directories(
    session_user_id: &str,
    parent: Option<&str>,
    env: Option<Arc<dyn Environment>>,
) -> Result<WorkspaceListResult> {
    let policy = policy_for_user(session_user_id)?;
    let parent_path = resolve_list_parent(&policy, parent)?;
    ensure_allowed_selection(session_user_id, &policy, &parent_path)?;

    let entries = match env {
        Some(env) if env.kind() != crate::env::EnvKind::Local => {
            list_via_env(env.as_ref(), &parent_path)?
        }
        _ => list_via_host(&parent_path)?,
    };
    Ok(WorkspaceListResult {
        parent: parent_path,
        entries,
    })
}

/// Create a directory under an allowed selectable root.
pub fn create_directory(
    session_user_id: &str,
    parent: &str,
    name: &str,
    env: Option<Arc<dyn Environment>>,
) -> Result<WorkspaceDirEntry> {
    let policy = policy_for_user(session_user_id)?;
    if !policy.can_create_directories {
        bail!("当前账号不允许创建目录");
    }
    let name = name.trim();
    validate_new_dir_name(name, &policy)?;
    let parent_path = parent.trim().to_string();
    ensure_allowed_selection(session_user_id, &policy, &parent_path)?;
    let child = join_display_path(&parent_path, name);

    match env {
        Some(env) if env.kind() != crate::env::EnvKind::Local => {
            env.create_dir_all(Path::new(&child))
                .with_context(|| format!("create sandbox dir {child}"))?;
        }
        _ => {
            std::fs::create_dir_all(&child)
                .with_context(|| format!("create host dir {child}"))?;
        }
    }
    log::info!(
        "workspace_service: created directory user_key={} path={child}",
        user_key(session_user_id)
    );
    Ok(WorkspaceDirEntry {
        name: name.to_string(),
        path: child,
        is_dir: true,
    })
}

/// Validate and accept a workspace selection.
pub fn select_workspace(
    session_user_id: &str,
    selection: &WorkspaceSelection,
    env: Option<Arc<dyn Environment>>,
) -> Result<WorkspaceSelectResult> {
    let policy = policy_for_user(session_user_id)?;
    let path = selection.path.trim().to_string();
    ensure_allowed_selection(session_user_id, &policy, &path)?;

    match env {
        Some(env) if env.kind() != crate::env::EnvKind::Local => {
            if !env.exists(Path::new(&path)) {
                bail!("目录不存在: {path}");
            }
            let meta = env.metadata(Path::new(&path))?;
            if !meta.is_dir {
                bail!("工作区必须是目录: {path}");
            }
        }
        _ => {
            let p = PathBuf::from(&path);
            if !p.is_dir() {
                bail!("工作区必须是已存在的目录: {path}");
            }
        }
    }

    log::info!(
        "workspace_service: selected workspace user_key={} path={path}",
        user_key(session_user_id)
    );
    Ok(WorkspaceSelectResult {
        path: path.clone(),
        display_path: path,
    })
}

/// Build a ResolvedWorkspace for a conversation/project path.
pub fn resolve_workspace(
    session_user_id: &str,
    workspace_path: &str,
    env: Option<Arc<dyn Environment>>,
) -> Result<ResolvedWorkspace> {
    let policy = policy_for_user(session_user_id)?;
    let path = workspace_path.trim().to_string();
    ensure_allowed_selection(session_user_id, &policy, &path)?;
    let data_root = match env.as_ref().map(|e| e.kind()) {
        Some(crate::env::EnvKind::Local) | None => None,
        Some(_) => Some(PathBuf::from(SANDBOX_DATA_ROOT)),
    };
    Ok(ResolvedWorkspace {
        owner_user_key: user_key(session_user_id),
        data_root,
        workspace_root: PathBuf::from(&path),
        active_cwd: PathBuf::from(&path),
    })
}

/// Acquire the Environment used for sandbox directory browsing when needed.
pub fn browse_env_for_user(
    conversation_id: &str,
    session_user_id: &str,
) -> Result<Option<Arc<dyn Environment>>> {
    let cfg = ProviderConfig::from_env()?;
    let uid = session_user_id.trim();
    let resolved = cfg.resolve(if uid.is_empty() { None } else { Some(uid) })?;
    if resolved.tier == Tier::Local {
        return Ok(None);
    }
    crate::env::session_env::resolve_session_env(
        &cfg,
        conversation_id,
        session_user_id,
        None,
    )
}

fn resolve_list_parent(policy: &WorkspacePolicy, parent: Option<&str>) -> Result<String> {
    let raw = parent.map(str::trim).filter(|s| !s.is_empty());
    match raw {
        Some(path) => Ok(path.to_string()),
        None => policy
            .selectable_roots
            .first()
            .cloned()
            .ok_or_else(|| anyhow!("当前账号没有可选工作区根")),
    }
}

fn ensure_allowed_selection(
    session_user_id: &str,
    policy: &WorkspacePolicy,
    path: &str,
) -> Result<()> {
    let cfg = ProviderConfig::from_env()?;
    let uid = session_user_id.trim();
    let resolved = cfg.resolve(if uid.is_empty() { None } else { Some(uid) })?;
    match resolved.tier {
        Tier::Local => ensure_local_selection(policy, path),
        Tier::Headless | Tier::Desktop => {
            if looks_like_host_absolute_path(path) {
                log::warn!(
                    "workspace_service: rejected host path for sandbox user_key={}",
                    user_key(session_user_id)
                );
            }
            reject_sandbox_host_path(path)?;
            Ok(())
        }
    }
}

fn ensure_local_selection(policy: &WorkspacePolicy, path: &str) -> Result<()> {
    let candidate = Path::new(path.trim());
    if !candidate.is_absolute() {
        bail!("本地工作区必须是绝对路径");
    }
    for root in &policy.selectable_roots {
        if ensure_descendant_path(Path::new(root), candidate).is_ok() {
            return Ok(());
        }
    }
    // Local Client may authorize an arbitrary picked directory; require it exists.
    if candidate.is_dir() {
        return Ok(());
    }
    bail!("路径不在可选择范围内: {path}");
}

fn validate_new_dir_name(name: &str, policy: &WorkspacePolicy) -> Result<()> {
    if name.is_empty() {
        bail!("目录名不能为空");
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        bail!("目录名非法");
    }
    if policy.reserved_names.iter().any(|n| n == name) {
        bail!("目录名属于保留名: {name}");
    }
    Ok(())
}

fn join_display_path(parent: &str, name: &str) -> String {
    let parent = parent.trim_end_matches('/');
    if parent.is_empty() {
        format!("/{name}")
    } else {
        format!("{parent}/{name}")
    }
}

fn list_via_host(parent: &str) -> Result<Vec<WorkspaceDirEntry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(parent).with_context(|| format!("list {parent}"))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let is_dir = entry.file_type()?.is_dir();
        if !is_dir {
            continue;
        }
        entries.push(WorkspaceDirEntry {
            path: join_display_path(parent, &name),
            name,
            is_dir,
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

fn list_via_env(env: &dyn Environment, parent: &str) -> Result<Vec<WorkspaceDirEntry>> {
    let mut entries = Vec::new();
    for entry in env.list_dir(Path::new(parent))? {
        if entry.name.starts_with('.') || !entry.is_dir {
            continue;
        }
        entries.push(WorkspaceDirEntry {
            path: join_display_path(parent, &entry.name),
            name: entry.name,
            is_dir: true,
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

/// Host-only helper used by local tests without a full Environment.
#[cfg(test)]
pub fn local_env_for_tests(root: &Path) -> Arc<dyn Environment> {
    Arc::new(LocalEnv::new(root)) as Arc<dyn Environment>
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_policy_roots_are_data_workspaces() {
        let policy = sandbox_workspace_policy();
        assert_eq!(policy.selectable_roots, vec![crate::workspace_resolve::SANDBOX_WORKSPACES_ROOT]);
        assert!(policy.reserved_names.contains(&"attachments".into()));
    }

    #[test]
    fn join_paths_keep_posix_style() {
        assert_eq!(
            join_display_path("/data/workspaces", "demo"),
            "/data/workspaces/demo"
        );
    }

    #[test]
    fn reserved_names_blocked() {
        let policy = sandbox_workspace_policy();
        assert!(validate_new_dir_name("attachments", &policy).is_err());
        assert!(validate_new_dir_name("demo", &policy).is_ok());
    }
}
