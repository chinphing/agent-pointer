//! Session-level wiring for managed worktrees.
//!
//! The manager stays generic; this module owns the app policy:
//! - one worktree per conversation, bound to that conversation id;
//! - created from the conversation's current workspace (the source repository);
//! - pruned after creation, but never while its conversation still exists.

use anyhow::{bail, Result};
use std::path::PathBuf;

use super::{CreateWorktree, ManagedWorktree, WorktreeInfo, WorktreeManager};

/// Create a checkout for `conversation_id` from `source_workspace`.
///
/// `base` defaults to the source checkout's current `HEAD`.
pub fn create_for_conversation(
    conversation_id: &str,
    source_workspace: &str,
    base: Option<&str>,
) -> Result<ManagedWorktree> {
    let manager = WorktreeManager::for_app()?;
    create_with_manager(
        &manager,
        conversation_id,
        source_workspace,
        base,
        conversation_still_exists,
    )
}

/// Same as [`create_for_conversation`] with an explicit manager and liveness probe.
///
/// `keep_owner` answers "is this owning conversation still alive?" and decides
/// whether an old checkout survives pruning.
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

/// Managed checkouts that belong to the repository of `source_workspace`.
pub fn list_for_workspace(source_workspace: &str) -> Result<Vec<ManagedWorktree>> {
    let source = source_workspace.trim();
    if source.is_empty() {
        bail!("缺少项目目录，无法列出 worktree");
    }
    WorktreeManager::for_app()?.list(&PathBuf::from(source))
}

/// Remove one managed checkout; refuses uncommitted or ignored content.
pub fn remove_for_workspace(source_workspace: &str, root: &str) -> Result<()> {
    let source = source_workspace.trim();
    if source.is_empty() || root.trim().is_empty() {
        bail!("缺少项目目录或 worktree 路径");
    }
    WorktreeManager::for_app()?.remove(&PathBuf::from(source), &PathBuf::from(root.trim()))
}

/// Describe `workspace_root` when it is a linked worktree.
pub fn inspect_workspace(workspace_root: &str) -> Result<Option<WorktreeInfo>> {
    let path = workspace_root.trim();
    if path.is_empty() {
        return Ok(None);
    }
    super::inspect(&PathBuf::from(path))
}

/// Fail-safe: an unreadable store keeps the checkout instead of deleting it.
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
}
