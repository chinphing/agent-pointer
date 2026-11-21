//! Allocates managed-worktree buckets and removes empty ones.
//!
//! Layout: `<root>/<4-hex bucket>/<repository name>/`. The random bucket keeps
//! two worktrees of the same repository from colliding on disk and makes the
//! managed set recognizable without a database.

use anyhow::{bail, Result};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::git::{create_dir_exclusive, ensure_dir_all, remove_empty_dir, write_bytes};

pub(crate) fn allocate_worktree_root(root: &Path, repository_name: &OsStr) -> Result<PathBuf> {
    ensure_dir_all(root)?;

    #[cfg(target_os = "macos")]
    {
        // Keep Spotlight from indexing hundreds of throwaway checkouts.
        let marker = root.join(".metadata_never_index");
        write_bytes(&marker, b"")?;
    }

    for _ in 0..=u16::MAX {
        let identifier = uuid::Uuid::new_v4().simple().to_string();
        let bucket = root.join(&identifier[..4]);
        if create_dir_exclusive(&bucket)? {
            return Ok(bucket.join(repository_name));
        }
    }

    bail!("all managed worktree identifiers are in use")
}

pub(crate) fn remove_empty_bucket(checkout: &Path) {
    if let Some(bucket) = checkout.parent() {
        remove_empty_dir(bucket);
    }
}
