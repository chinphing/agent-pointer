//! Conversation ownership for managed worktrees.
//!
//! Each managed checkout stores a versioned `pointer-thread.json` inside its own
//! git administration directory (`git rev-parse --git-path`), not in the working
//! tree — so it never shows up in `git status` and survives branch switches.
//!
//! Binding is atomic: publishing uses `persist_noclobber`, so two conversations
//! racing for the same checkout cannot both win.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

use super::git::git_path;

const OWNER_FILENAME: &str = "pointer-thread.json";
const OWNER_VERSION: u8 = 1;

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnerRecord {
    version: u8,
    owner_thread_id: String,
}

pub(crate) fn owner(checkout: &Path) -> Result<Option<String>> {
    let path = metadata_path(checkout)?;
    let contents = match fs::read(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("cannot read worktree owner at {}", path.display()));
        }
    };
    let record: OwnerRecord = serde_json::from_slice(&contents)
        .with_context(|| format!("invalid worktree owner at {}", path.display()))?;
    if record.version != OWNER_VERSION || record.owner_thread_id.is_empty() {
        bail!("invalid worktree owner at {}", path.display());
    }
    Ok(Some(record.owner_thread_id))
}

pub(crate) fn bind_thread(checkout: &Path, thread_id: &str) -> Result<()> {
    if thread_id.trim().is_empty() {
        bail!("worktree owner id cannot be empty");
    }
    if let Some(existing) = owner(checkout)? {
        if existing == thread_id {
            return Ok(());
        }
        bail!("worktree already belongs to {existing}");
    }

    let path = metadata_path(checkout)?;
    let directory = path.parent().context("worktree metadata has no parent")?;
    let mut temporary = NamedTempFile::new_in(directory)
        .with_context(|| format!("cannot create worktree metadata in {}", directory.display()))?;
    serde_json::to_writer(
        &mut temporary,
        &OwnerRecord {
            version: OWNER_VERSION,
            owner_thread_id: thread_id.to_owned(),
        },
    )?;
    temporary.flush()?;

    match temporary.persist_noclobber(&path) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            if owner(checkout)?.as_deref() == Some(thread_id) {
                Ok(())
            } else {
                bail!("worktree was concurrently assigned to another conversation")
            }
        }
        Err(error) => Err(error.error)
            .with_context(|| format!("cannot write worktree owner at {}", path.display())),
    }
}

fn metadata_path(checkout: &Path) -> Result<PathBuf> {
    let relative = git_path(checkout, ["rev-parse", "--git-path", OWNER_FILENAME])?;
    Ok(if relative.is_absolute() {
        relative
    } else {
        checkout.join(relative)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::test_support::init_repo;

    #[test]
    fn bind_is_idempotent_and_rejects_another_owner() {
        let (temp, repo) = init_repo();
        let manager = crate::worktree::WorktreeManager::with_root(temp.path().join("wt"));
        let created = manager
            .create(&crate::worktree::CreateWorktree {
                source_cwd: repo.clone(),
                base: None,
            })
            .unwrap();

        manager.bind_thread(&created.root, "conv-a").unwrap();
        manager.bind_thread(&created.root, "conv-a").unwrap();
        assert_eq!(
            manager.owner(&created.root).unwrap().as_deref(),
            Some("conv-a")
        );
        assert!(manager.bind_thread(&created.root, "conv-b").is_err());
        assert!(manager.bind_thread(&created.root, "  ").is_err());
    }

    #[test]
    fn owner_record_lives_outside_the_worktree() {
        let (temp, repo) = init_repo();
        let manager = crate::worktree::WorktreeManager::with_root(temp.path().join("wt"));
        let created = manager
            .create(&crate::worktree::CreateWorktree {
                source_cwd: repo,
                base: None,
            })
            .unwrap();
        manager.bind_thread(&created.root, "conv-a").unwrap();
        assert!(!created.root.join(OWNER_FILENAME).exists());
    }
}
