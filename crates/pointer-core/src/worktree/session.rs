//! Session-level wiring for managed worktrees.
//!
//! The manager stays generic; this module owns the app policy:
//! - one worktree per conversation, bound to that conversation id;
//! - created from the conversation's current workspace (the source repository);
//! - pruned after creation, but never while its conversation still exists.

use anyhow::{bail, Result};
use std::path::PathBuf;

use std::sync::Arc;

use super::git::with_git_env;
use super::{CreateWorktree, ManagedWorktree, WorktreeInfo, WorktreeManager};

/// Create a checkout for `conversation_id`.
///
/// Source is resolved from the conversation/project workspace. Clients must not
/// pass a host absolute path for sandbox accounts; an optional hint is only used
/// when the conversation has no persisted workspace yet (local).
///
/// `base` defaults to the source checkout's current `HEAD`.
pub fn create_for_conversation(
    conversation_id: &str,
    source_workspace_hint: Option<&str>,
    base: Option<&str>,
) -> Result<ManagedWorktree> {
    let source = resolve_source_for_conversation(conversation_id, source_workspace_hint)?;
    if crate::workspace_resolve::looks_like_sandbox_guest_path(&source) {
        let env = crate::env::session_env::env_for_conversation(conversation_id).ok_or_else(|| {
            anyhow::anyhow!(
                "沙箱会话还没有执行环境，无法在容器里创建 worktree（conversation={conversation_id}）"
            )
        })?;
        if env.is_denied() || env.path_map().is_host_passthrough() {
            bail!("沙箱会话的执行环境不可用，无法在容器里创建 worktree");
        }
        log::info!(
            "worktree: git via session environment conversation={conversation_id} source={source}"
        );
        let manager = WorktreeManager::with_root(PathBuf::from("/data/worktrees"));
        let env: Arc<dyn crate::env::Environment> = env;
        return with_git_env(env, || {
            create_with_manager(
                &manager,
                conversation_id,
                &source,
                base,
                conversation_still_exists,
            )
        });
    }
    reject_client_host_source_for_sandbox(conversation_id, &source)?;
    let manager = manager_for_conversation(conversation_id)?;
    create_with_manager(
        &manager,
        conversation_id,
        &source,
        base,
        conversation_still_exists,
    )
}

/// Same as [`create_for_conversation`] with an explicit manager and liveness probe.
///
/// `keep_owner` answers "is this owning conversation still alive?" and decides
/// whether an old checkout survives pruning.
///
/// `source_workspace` is a host path on the local tier, or a guest `/data/...` path
/// when [`super::git::with_git_env`] has bound the session environment.
pub fn create_with_manager(
    manager: &WorktreeManager,
    conversation_id: &str,
    source_workspace: &str,
    base: Option<&str>,
    keep_owner: impl FnMut(&str) -> bool,
) -> Result<ManagedWorktree> {
    let conversation_id = conversation_id.trim();
    if conversation_id.is_empty() {
        bail!("缺少会话 id，无法绑定 worktree");
    }
    let source = source_workspace.trim();
    if source.is_empty() {
        bail!("请先选择项目目录，再创建 worktree");
    }
    // Local sources are host paths. Guest `/data/...` sources run only while
    // [`super::git::with_git_env`] has bound the session environment.
    let created = manager.create(&CreateWorktree {
        source_cwd: PathBuf::from(source),
        base: base.map(str::to_owned),
    })?;
    manager.bind_thread(&created.root, conversation_id)?;

    if manager.settings().auto_cleanup_enabled {
        let keep = manager.settings().keep_count;
        if let Err(error) = manager.prune(&created.source_root, keep, keep_owner) {
            // Cleanup is best-effort: a failed prune must never fail the create.
            log::warn!("worktree: prune after create failed: {error:#}");
        }
    }

    Ok(created)
}

/// Resolve the Environment-namespace source path for a conversation.
fn resolve_source_for_conversation(
    conversation_id: &str,
    hint: Option<&str>,
) -> Result<String> {
    if let Ok(store) = crate::conversation_store::global_store() {
        if let Ok(workspace) = store.workspace_root(conversation_id) {
            let trimmed = workspace.trim();
            if !trimmed.is_empty() {
                log::info!(
                    "worktree: source from conversation store conversation={conversation_id} path={trimmed}"
                );
                return Ok(trimmed.to_string());
            }
        }
    }
    if let Some(hint) = hint.map(str::trim).filter(|s| !s.is_empty()) {
        log::info!(
            "worktree: source from client hint conversation={conversation_id} path={hint}"
        );
        return Ok(hint.to_string());
    }
    bail!("请先选择项目目录，再创建 worktree");
}

fn manager_for_conversation(conversation_id: &str) -> Result<WorktreeManager> {
    let uid = crate::conversation_store::global_store()
        .ok()
        .and_then(|store| store.session_user_id(conversation_id).ok())
        .unwrap_or_default();
    // Sandbox users: keep managed trees under the user data volume so guest
    // path `/data/worktrees/...` stays consistent. Local keeps app_data/worktrees.
    let cfg = crate::env::provider::ProviderConfig::from_env().ok();
    let is_sandbox = cfg
        .and_then(|c| {
            c.resolve(if uid.trim().is_empty() {
                None
            } else {
                Some(uid.as_str())
            })
            .ok()
        })
        .map(|r| r.tier != crate::env::provider::Tier::Local)
        .unwrap_or(false);
    if is_sandbox {
        let data = crate::user_data_paths::user_data_host_root(&uid, conversation_id)?;
        let root = data.join("worktrees");
        std::fs::create_dir_all(&root)?;
        return Ok(WorktreeManager::with_root(root));
    }
    WorktreeManager::for_app()
}

/// Managed checkouts that belong to the repository of `source_workspace`.
pub fn list_for_workspace(source_workspace: &str) -> Result<Vec<ManagedWorktree>> {
    let source = source_workspace.trim();
    if source.is_empty() {
        bail!("缺少项目目录，无法列出 worktree");
    }
    if crate::workspace_resolve::looks_like_sandbox_guest_path(source) {
        bail!("沙箱项目的 worktree 需要在会话环境里列出");
    }
    WorktreeManager::for_app()?.list(&PathBuf::from(source))
}

/// Remove one managed checkout; refuses uncommitted or ignored content.
pub fn remove_for_workspace(source_workspace: &str, root: &str) -> Result<()> {
    let source = source_workspace.trim();
    if source.is_empty() || root.trim().is_empty() {
        bail!("缺少项目目录或 worktree 路径");
    }
    if crate::workspace_resolve::looks_like_sandbox_guest_path(source)
        || crate::workspace_resolve::looks_like_sandbox_guest_path(root)
    {
        bail!("沙箱项目的 worktree 需要在会话环境里删除");
    }
    WorktreeManager::for_app()?.remove(&PathBuf::from(source), &PathBuf::from(root.trim()))
}

/// Describe `workspace_root` when it is a linked worktree.
///
/// Guest `/data/...` paths run git in the conversation's session environment.
pub fn inspect_workspace(
    workspace_root: &str,
    conversation_id: Option<&str>,
) -> Result<Option<WorktreeInfo>> {
    let path = workspace_root.trim();
    if path.is_empty() {
        return Ok(None);
    }
    let path_buf = PathBuf::from(path);
    if !crate::workspace_resolve::looks_like_sandbox_guest_path(path) {
        return super::inspect(&path_buf);
    }
    let conversation_id = conversation_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| anyhow::anyhow!("沙箱工作区需要会话才能检查 worktree"))?;
    let env = crate::env::session_env::env_for_conversation(conversation_id).ok_or_else(|| {
        anyhow::anyhow!("沙箱会话还没有执行环境，无法检查 worktree（conversation={conversation_id}）")
    })?;
    if env.is_denied() || env.path_map().is_host_passthrough() {
        bail!("沙箱会话的执行环境不可用，无法检查 worktree");
    }
    log::info!(
        "worktree: inspect via session environment conversation={conversation_id} path={path}"
    );
    let env: Arc<dyn crate::env::Environment> = env;
    with_git_env(env, || {
        super::inspect_with_root(&path_buf, Some(std::path::Path::new("/data/worktrees")))
    })
}

/// Remove the managed worktree that belongs to a conversation that is going away.
///
/// Returns the removed root when a tree was actually deleted. Deliberately quiet
/// about non-worktree workspaces (projects, sandboxes, already-deleted trees).
/// A tree owned by a different conversation is never touched; a tree with
/// uncommitted or ignored content is refused by [`WorktreeManager::remove`] and
/// the error is returned so the caller can keep the conversation deletion and log.
pub fn remove_for_conversation(
    conversation_id: &str,
    workspace_root: &str,
) -> Result<Option<PathBuf>> {
    if crate::workspace_resolve::looks_like_sandbox_guest_path(workspace_root) {
        let env = crate::env::session_env::env_for_conversation(conversation_id).ok_or_else(|| {
            anyhow::anyhow!(
                "沙箱会话还没有执行环境，无法删除 worktree（conversation={conversation_id}）"
            )
        })?;
        if env.is_denied() || env.path_map().is_host_passthrough() {
            bail!("沙箱会话的执行环境不可用，无法删除 worktree");
        }
        log::info!(
            "worktree: remove via session environment conversation={conversation_id} path={workspace_root}"
        );
        let manager = WorktreeManager::with_root(PathBuf::from("/data/worktrees"));
        let env: Arc<dyn crate::env::Environment> = env;
        return with_git_env(env, || {
            remove_for_conversation_with_manager(&manager, conversation_id, workspace_root)
        });
    }
    let manager = WorktreeManager::for_app()?;
    remove_for_conversation_with_manager(&manager, conversation_id, workspace_root)
}

/// Same as [`remove_for_conversation`] with an explicit manager (tests).
pub fn remove_for_conversation_with_manager(
    manager: &WorktreeManager,
    conversation_id: &str,
    workspace_root: &str,
) -> Result<Option<PathBuf>> {
    let conversation_id = conversation_id.trim();
    let path = workspace_root.trim();
    if conversation_id.is_empty() || path.is_empty() {
        return Ok(None);
    }
    let root = PathBuf::from(path);
    if !manager.is_managed_path(&root) {
        return Ok(None);
    }
    // Never delete a checkout another conversation owns.
    if let Some(owner) = manager.owner(&root)? {
        if owner != conversation_id {
            log::info!(
                "worktree: {} belongs to {owner}, not {conversation_id}; leaving it",
                root.display()
            );
            return Ok(None);
        }
    }
    let Some(info) = super::inspect(&root)? else {
        // The checkout is already gone from disk.
        return Ok(None);
    };
    manager.remove(&info.source_root, &root)?;
    Ok(Some(info.root))
}

/// Fail-safe: an unreadable store keeps the checkout instead of deleting it.

/// Sandbox-tier conversations must not create worktrees from a client-supplied
/// host path. The source must later come from the Environment project path.
fn reject_client_host_source_for_sandbox(conversation_id: &str, source: &str) -> Result<()> {
    let session_user_id = crate::conversation_store::global_store()
        .ok()
        .and_then(|store| store.session_user_id(conversation_id).ok())
        .unwrap_or_default();
    let cfg = match crate::env::provider::ProviderConfig::from_env() {
        Ok(cfg) => cfg,
        Err(error) => {
            log::warn!(
                "worktree: failed to read environment config while guarding source conversation={conversation_id}: {error:#}"
            );
            return Ok(());
        }
    };
    let uid = session_user_id.trim();
    let resolved = match cfg.resolve(if uid.is_empty() { None } else { Some(uid) }) {
        Ok(resolved) => resolved,
        Err(error) => {
            log::warn!(
                "worktree: failed to resolve tier while guarding source conversation={conversation_id}: {error:#}"
            );
            return Ok(());
        }
    };
    if resolved.tier == crate::env::provider::Tier::Local {
        return Ok(());
    }
    log::warn!(
        "worktree: rejected host source for sandbox conversation conversation_id={conversation_id} tier={} source={source}",
        resolved.tier.as_str()
    );
    bail!("沙箱会话不能使用宿主路径创建 worktree，请选择沙箱内的项目目录");
}

fn conversation_still_exists(conversation_id: &str) -> bool {
    crate::conversation_store::global_store()
        .ok()
        .and_then(|store| store.conversation_exists(conversation_id).ok())
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::test_support::init_repo;
    use crate::worktree::WorktreeSettings;
    use std::time::Duration;

    fn manager_keeping(temp: &tempfile::TempDir, keep: usize) -> WorktreeManager {
        let mut settings = WorktreeSettings::with_root(temp.path().join("managed"));
        settings.keep_count = keep;
        WorktreeManager::new(settings)
    }

    #[test]
    fn create_binds_the_conversation_to_the_checkout() {
        let (temp, repo) = init_repo();
        let manager = manager_keeping(&temp, 15);
        let created = create_with_manager(
            &manager,
            "conv-a",
            repo.to_str().unwrap(),
            None,
            |_| false,
        )
        .unwrap();
        assert!(created.cwd.is_dir());
        assert_eq!(manager.owner(&created.root).unwrap().as_deref(), Some("conv-a"));
        assert_eq!(created.base.as_deref(), Some("HEAD"));
    }

    #[test]
    fn create_rejects_blank_ids_and_paths() {
        let (temp, repo) = init_repo();
        let manager = manager_keeping(&temp, 15);
        assert!(create_with_manager(&manager, "  ", repo.to_str().unwrap(), None, |_| false).is_err());
        assert!(create_with_manager(&manager, "conv-a", "  ", None, |_| false).is_err());
    }

    #[test]
    fn prune_keeps_the_newest_plus_checkouts_with_live_conversations() {
        let (temp, repo) = init_repo();
        // keep=1 with the same liveness probe on every create: the newest checkout
        // survives by count, the middle one is pruned, and the oldest survives
        // because its conversation is still alive.
        let manager = manager_keeping(&temp, 1);
        let source = repo.to_str().unwrap().to_owned();
        let live = |owner: &str| owner == "conv-a";

        let oldest = create_with_manager(&manager, "conv-a", &source, None, live).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        let middle = create_with_manager(&manager, "conv-b", &source, None, live).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        let newest = create_with_manager(&manager, "conv-c", &source, None, live).unwrap();

        assert!(oldest.root.exists(), "live conversation keeps its checkout");
        assert!(!middle.root.exists(), "unowned old checkout is pruned");
        assert!(newest.root.exists(), "newest checkout is kept by count");
    }

    #[test]
    fn remove_for_conversation_deletes_the_owned_checkout() {
        let (temp, repo) = init_repo();
        let manager = manager_keeping(&temp, 15);
        let created =
            create_with_manager(&manager, "conv-a", repo.to_str().unwrap(), None, |_| false)
                .unwrap();

        let removed = remove_for_conversation_with_manager(
            &manager,
            "conv-a",
            created.cwd.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(removed.as_deref(), Some(created.root.as_path()));
        assert!(!created.root.exists());
    }

    #[test]
    fn remove_for_conversation_skips_other_owners_and_plain_directories() {
        let (temp, repo) = init_repo();
        let manager = manager_keeping(&temp, 15);
        let created =
            create_with_manager(&manager, "conv-a", repo.to_str().unwrap(), None, |_| false)
                .unwrap();

        // Another conversation must not be able to delete this checkout.
        assert!(remove_for_conversation_with_manager(
            &manager,
            "conv-b",
            created.cwd.to_str().unwrap()
        )
        .unwrap()
        .is_none());
        assert!(created.root.exists());

        // A plain project directory is never touched.
        assert!(remove_for_conversation_with_manager(
            &manager,
            "conv-a",
            repo.to_str().unwrap()
        )
        .unwrap()
        .is_none());
        assert!(repo.exists());
    }

    #[test]
    fn remove_for_conversation_refuses_uncommitted_changes() {
        let (temp, repo) = init_repo();
        let manager = manager_keeping(&temp, 15);
        let created =
            create_with_manager(&manager, "conv-a", repo.to_str().unwrap(), None, |_| false)
                .unwrap();
        std::fs::write(created.root.join("README.md"), "dirty").unwrap();

        assert!(remove_for_conversation_with_manager(
            &manager,
            "conv-a",
            created.cwd.to_str().unwrap()
        )
        .is_err());
        assert!(created.root.exists(), "dirty checkout must survive");
    }
}
