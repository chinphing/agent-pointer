//! Managed-worktree settings.
//!
//! Worktrees live under `{app_data}/worktrees/<4-hex bucket>/<repository name>/`
//! so they never pollute the user's repository and can be cleaned up as one tree.
//!
//! Decisions locked for the session-level worktree feature:
//! - scope: per conversation (not per sub-agent) — see `docs/zh-CN/design/session-worktree.md`
//! - base ref: the source checkout's current `HEAD`
//! - root: `{app_data}/worktrees/`
//! - auto cleanup: keep the newest [`DEFAULT_WORKTREE_KEEP_COUNT`] worktrees
//! - isolation strength: write-conflict isolation only (no sandboxing)

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Number of managed worktrees kept per repository before pruning the oldest.
pub const DEFAULT_WORKTREE_KEEP_COUNT: usize = 15;

/// Folder name under the application data directory.
pub const WORKTREES_DIR: &str = "worktrees";

/// Effective host-local settings for managed worktrees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeSettings {
    /// Absolute directory that contains the `{4hex}/{repo}` buckets.
    pub root: PathBuf,
    /// Prune old worktrees after a successful create.
    pub auto_cleanup_enabled: bool,
    /// How many worktrees to keep (newest by mtime).
    pub keep_count: usize,
}

impl WorktreeSettings {
    /// Settings for the running app: `{app_data}/worktrees`, cleanup on, keep 15.
    pub fn for_app() -> Result<Self> {
        let root = crate::storage::app_data_dir()
            .context("cannot resolve app data dir for managed worktrees")?
            .join(WORKTREES_DIR);
        Ok(Self::with_root(root))
    }

    /// Same defaults, but rooted at an explicit absolute directory (tests / overrides).
    pub fn with_root(root: PathBuf) -> Self {
        Self {
            root,
            auto_cleanup_enabled: true,
            keep_count: DEFAULT_WORKTREE_KEEP_COUNT,
        }
    }

    /// Reject a non-absolute root early: git would otherwise resolve it against `cwd`.
    pub fn validate(&self) -> Result<()> {
        if !self.root.is_absolute() {
            anyhow::bail!(
                "managed worktree root must be an absolute path: {}",
                self.root.display()
            );
        }
        if self.keep_count == 0 {
            anyhow::bail!("worktree keep count must be a positive integer");
        }
        Ok(())
    }
}

/// `{app_data}/worktrees/` for the current process.
pub fn default_root() -> Result<PathBuf> {
    Ok(crate::storage::app_data_dir()
        .context("cannot resolve app data dir for managed worktrees")?
        .join(WORKTREES_DIR))
}

/// Whether `path` sits under `root` using the managed `{4hex}/{repo}` layout.
pub fn has_managed_layout(root: &Path, checkout: &Path) -> bool {
    let Ok(relative) = checkout.strip_prefix(root) else {
        return false;
    };
    let mut components = relative.components();
    let Some(std::path::Component::Normal(bucket)) = components.next() else {
        return false;
    };
    let Some(bucket) = bucket.to_str() else {
        return false;
    };
    bucket.len() == 4
        && bucket.bytes().all(|byte| byte.is_ascii_hexdigit())
        && matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_layout_requires_bucket_and_repo() {
        let root = Path::new("/tmp/wt");
        assert!(has_managed_layout(root, Path::new("/tmp/wt/1a2b/repo")));
        assert!(!has_managed_layout(root, Path::new("/tmp/wt/repo")));
        assert!(!has_managed_layout(root, Path::new("/tmp/wt/zzzz/repo")));
        assert!(!has_managed_layout(root, Path::new("/tmp/wt/1a2b/repo/nested")));
        assert!(!has_managed_layout(root, Path::new("/elsewhere/1a2b/repo")));
    }

    #[test]
    fn rejects_relative_root_and_zero_keep() {
        let mut settings = WorktreeSettings::with_root(PathBuf::from("relative/wt"));
        assert!(settings.validate().is_err());
        settings.root = PathBuf::from("/tmp/wt");
        assert!(settings.validate().is_ok());
        settings.keep_count = 0;
        assert!(settings.validate().is_err());
    }
}
