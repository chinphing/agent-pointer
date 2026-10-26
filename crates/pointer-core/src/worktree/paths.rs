//! Allocates managed-worktree buckets and removes empty ones.
//!
//! Layout: `<root>/<4-hex bucket>/<repository name>/`. The random bucket keeps
//! two worktrees of the same repository from colliding on disk and makes the
//! managed set recognizable without a database.

use anyhow::{bail, Context, Result};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(crate) fn allocate_worktree_root(root: &Path, repository_name: &OsStr) -> Result<PathBuf> {
    fs::create_dir_all(root)
        .with_context(|| format!("cannot create worktree root {}", root.display()))?;

    #[cfg(target_os = "macos")]
    {
        // Keep Spotlight from indexing hundreds of throwaway checkouts.
        let marker = root.join(".metadata_never_index");
        fs::write(&marker, b"")
            .with_context(|| format!("cannot disable Spotlight indexing at {}", marker.display()))?;
    }

    for _ in 0..=u16::MAX {
        let identifier = uuid::Uuid::new_v4().simple().to_string();
        let bucket = root.join(&identifier[..4]);
        match fs::create_dir(&bucket) {
            Ok(()) => return Ok(bucket.join(repository_name)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("cannot create worktree bucket {}", bucket.display())
                });
            }
        }
    }

    bail!("all managed worktree identifiers are in use")
}

pub(crate) fn remove_empty_bucket(checkout: &Path) {
    if let Some(bucket) = checkout.parent() {
        let _ = fs::remove_dir(bucket);
    }
}
