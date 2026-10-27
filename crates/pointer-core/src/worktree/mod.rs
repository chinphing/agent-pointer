//! Managed git worktrees for session-level isolation.
//!
//! A conversation can be bound to its own checkout of the repository so its
//! edits never touch the user's dirty tree. The contract mirrors OpenAI Codex's
//! `codex-rs/worktree` crate:
//!
//! - `create` builds a **detached** checkout under
//!   `{app_data}/worktrees/<4-hex>/<repo>/`, never copying uncommitted changes;
//! - `list` only reports checkouts this app created (managed layout + linked
//!   `.git` file + matching common dir), so hand-made worktrees are untouched;
//! - `bind_thread` records which conversation owns a checkout;
//! - `remove` refuses dirty or ignored content and the current directory;
//! - `prune` keeps the newest N and preserves checkouts whose conversation is
//!   still alive.
//!
//! Design record: `docs/zh-CN/design/session-worktree.md`.

mod git;
mod metadata;
mod paths;
pub mod session;
mod settings;

#[cfg(test)]
pub(crate) mod test_support;

use anyhow::{bail, Context, Result};
use git::{git_output, git_path, git_stdout, repository_root, resolve_git_path, rev_parse_commit};
use git::{simplify_path, GitOperation};
use paths::{allocate_worktree_root, remove_empty_bucket};
use serde::Serialize;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

pub use settings::{
    default_root, has_managed_layout, WorktreeSettings, DEFAULT_WORKTREE_KEEP_COUNT, WORKTREES_DIR,
};

/// A checkout this app created, plus the cwd a conversation should start in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedWorktree {
    /// Checkout root (`<managed root>/<bucket>/<repo>`).
    pub root: PathBuf,
    /// Working directory inside the checkout, mirroring the source subdirectory.
    pub cwd: PathBuf,
    /// Repository root of the source checkout.
    pub source_root: PathBuf,
    /// Directory the user started the session from.
    pub source_cwd: PathBuf,
    pub head_sha: String,
    /// Branch label when the checkout is attached; `None` means detached.
    pub branch: Option<String>,
    /// Ref requested at creation (`HEAD` when the default was used).
    pub base: Option<String>,
}

/// What to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateWorktree {
    /// Directory inside the source repository (usually the session workspace).
    pub source_cwd: PathBuf,
    /// Revision to check out; defaults to the source checkout's current `HEAD`.
    pub base: Option<String>,
}

/// Result of pruning old managed checkouts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PruneReport {
    pub removed: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

/// Stateless description of any linked worktree, managed or not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    pub root: PathBuf,
    pub cwd: PathBuf,
    /// Main checkout that owns the shared git directory.
    pub source_root: PathBuf,
    pub head_sha: String,
    pub branch: Option<String>,
    pub detached: bool,
    /// True only when the checkout lives under this app's managed root.
    pub managed: bool,
}

/// Creates, lists and removes managed worktrees.
#[derive(Clone, Debug)]
pub struct WorktreeManager {
    settings: WorktreeSettings,
}

impl WorktreeManager {
    pub fn new(settings: WorktreeSettings) -> Self {
        Self { settings }
    }

    /// Manager for the running app (`{app_data}/worktrees`, keep 15, cleanup on).
    pub fn for_app() -> Result<Self> {
        Ok(Self::new(WorktreeSettings::for_app()?))
    }

    /// Manager rooted at an explicit directory with the app defaults.
    pub fn with_root(root: PathBuf) -> Self {
        Self::new(WorktreeSettings::with_root(root))
    }

    pub fn settings(&self) -> &WorktreeSettings {
        &self.settings
    }

    /// Create a detached checkout of the repository that contains `source_cwd`.
    pub fn create(&self, request: &CreateWorktree) -> Result<ManagedWorktree> {
        self.settings.validate()?;

        let source_cwd = fs::canonicalize(&request.source_cwd)
            .with_context(|| format!("cannot resolve {}", request.source_cwd.display()))?;
        let source_root = repository_root(&source_cwd)?;
        let relative_cwd = source_cwd
            .strip_prefix(&source_root)
            .context("working directory is outside the repository root")?
            .to_path_buf();
        let repository_name = source_root
            .file_name()
            .context("repository root has no directory name")?
            .to_os_string();

        let base_ref = request
            .base
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("HEAD");
        let head_sha = rev_parse_commit(&source_root, base_ref)?;
        let root = allocate_worktree_root(&self.settings.root, &repository_name)?;

        let args: Vec<&OsStr> = vec![
            OsStr::new("worktree"),
            OsStr::new("add"),
            OsStr::new("--detach"),
            OsStr::new("--no-checkout"),
            root.as_os_str(),
            OsStr::new(head_sha.as_str()),
        ];
        if let Err(error) = git_output(&source_root, GitOperation::WorkingTree, args) {
            remove_empty_bucket(&root);
            return Err(error).context("cannot create managed worktree");
        }

        // Pin this checkout's working tree, then materialize the files. Filters are
        // discovered per destination so a worktree-only `.gitattributes` cannot run
        // repository-configured smudge drivers.
        let populated = (|| -> Result<()> {
            let config = git_path(
                &root,
                [
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    "config.worktree",
                ],
            )?;
            let config_args: Vec<&OsStr> = vec![
                OsStr::new("config"),
                OsStr::new("--file"),
                config.as_os_str(),
                OsStr::new("core.worktree"),
                root.as_os_str(),
            ];
            git_output(&root, GitOperation::Metadata, config_args)?;
            let reset_args: Vec<&OsStr> = vec![
                OsStr::new("--work-tree=."),
                OsStr::new("reset"),
                OsStr::new("--hard"),
                OsStr::new("--no-recurse-submodules"),
                OsStr::new(head_sha.as_str()),
            ];
            git_output(&root, GitOperation::WorkingTree, reset_args)?;
            Ok(())
        })();
        if let Err(error) = populated {
            remove_worktree(&source_root, &root)
                .context("cannot roll back an incomplete managed worktree")?;
            return Err(error).context("cannot populate managed worktree");
        }

        let cwd = root.join(&relative_cwd);
        if !is_safe_worktree_cwd(&root, &cwd) {
            remove_worktree(&source_root, &root)
                .context("cannot roll back a worktree missing its working directory")?;
            bail!(
                "requested base does not contain a safe working directory {}",
                relative_cwd.display()
            );
        }

        // Report canonical paths: macOS temp dirs (and user-chosen symlinks) resolve
        // to a different prefix, and every later comparison (`list`, `remove`,
        // ownership lookups) works on canonical paths.
        let canonical_root = fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
        let canonical_cwd = canonical_root.join(&relative_cwd);
        let canonical_source = fs::canonicalize(&source_root).unwrap_or_else(|_| source_root.clone());

        Ok(ManagedWorktree {
            root: simplify_path(&canonical_root),
            cwd: simplify_path(&canonical_cwd),
            source_root: simplify_path(&canonical_source),
            source_cwd: simplify_path(&source_cwd),
            head_sha,
            branch: None,
            base: Some(base_ref.to_owned()),
        })
    }

    /// Managed checkouts of the repository that contains `source_cwd`.
    pub fn list(&self, source_cwd: &Path) -> Result<Vec<ManagedWorktree>> {
        let source_cwd = fs::canonicalize(source_cwd)
            .with_context(|| format!("cannot resolve {}", source_cwd.display()))?;
        let source_root = repository_root(&source_cwd)?;
        let relative_cwd = source_cwd
            .strip_prefix(&source_root)
            .context("working directory is outside the repository root")?
            .to_path_buf();
        let managed_root = fs::canonicalize(&self.settings.root)
            .unwrap_or_else(|_| self.settings.root.clone());
        let source_common_dir = resolve_git_path(&source_root, "--git-common-dir")?;
        let output = git_output(
            &source_cwd,
            GitOperation::Metadata,
            ["worktree", "list", "--porcelain", "-z"],
        )?;

        let mut worktrees = Vec::new();
        for (root, head_sha, branch) in parse_worktree_list(&output.stdout) {
            let Some(bucket) = root.parent() else {
                continue;
            };
            // Reject aliases: both the checkout and its bucket must be real directories.
            if !bucket.is_dir() {
                continue;
            }
            let canonical_root = fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
            if !has_managed_layout(&managed_root, &canonical_root) {
                continue;
            }
            let linked_git_file = fs::symlink_metadata(canonical_root.join(".git"))
                .is_ok_and(|metadata| metadata.file_type().is_file());
            if !linked_git_file {
                continue;
            }
            if resolve_git_path(&canonical_root, "--git-common-dir").ok().as_ref()
                != Some(&source_common_dir)
            {
                continue;
            }
            let cwd = canonical_root.join(&relative_cwd);
            if !is_safe_worktree_cwd(&canonical_root, &cwd) {
                continue;
            }
            let Some(head_sha) = head_sha else {
                log::warn!(
                    "worktree: managed checkout has no HEAD: {}",
                    canonical_root.display()
                );
                continue;
            };
            worktrees.push(ManagedWorktree {
                root: simplify_path(&canonical_root),
                cwd: simplify_path(&cwd),
                source_root: simplify_path(&source_root),
                source_cwd: simplify_path(&source_cwd),
                head_sha,
                branch,
                base: None,
            });
        }
        worktrees.sort_by(|left, right| left.root.cmp(&right.root));
        Ok(worktrees)
    }

    /// Remove one managed checkout, refusing anything with local content.
    pub fn remove(&self, source_cwd: &Path, root: &Path) -> Result<()> {
        let target = fs::canonicalize(root)
            .with_context(|| format!("cannot resolve worktree {}", root.display()))?;
        let checkout = self
            .list(source_cwd)?
            .into_iter()
            .find(|checkout| checkout.root == simplify_path(&target))
            .with_context(|| {
                format!(
                    "{} is not a managed worktree in this repository",
                    root.display()
                )
            })?;

        if let Ok(cwd) = std::env::current_dir() {
            if cwd.starts_with(&checkout.root) {
                bail!("switch to another checkout before deleting the current worktree");
            }
        }

        let status = git_stdout(
            &checkout.root,
            ["status", "--porcelain", "--untracked-files=normal"],
        )?;
        if !status.is_empty() {
            bail!("worktree has uncommitted changes; commit or discard them first");
        }
        let ignored = git_output(
            &checkout.root,
            GitOperation::WorkingTree,
            [
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "-z",
            ],
        )?;
        if !ignored.stdout.is_empty() {
            bail!("worktree contains ignored local files; remove them before deleting it");
        }

        git_output(
            &checkout.source_root,
            GitOperation::WorkingTree,
            vec![
                OsStr::new("worktree"),
                OsStr::new("remove"),
                checkout.root.as_os_str(),
            ],
        )?;
        remove_empty_bucket(&checkout.root);
        Ok(())
    }

    /// Record which conversation owns a checkout.
    pub fn bind_thread(&self, checkout: &Path, thread_id: &str) -> Result<()> {
        metadata::bind_thread(&self.managed_checkout(checkout)?, thread_id)
    }

    /// Owning conversation id, when one was bound.
    pub fn owner(&self, checkout: &Path) -> Result<Option<String>> {
        metadata::owner(&self.managed_checkout(checkout)?)
    }

    /// Keep the newest `keep` checkouts; drop the rest unless their conversation is alive.
    ///
    /// `keep_owner` returns `true` when an owning conversation still exists — those
    /// checkouts are preserved regardless of age so a resumed session keeps its tree.
    pub fn prune(
        &self,
        source_cwd: &Path,
        keep: usize,
        mut keep_owner: impl FnMut(&str) -> bool,
    ) -> Result<PruneReport> {
        let mut worktrees = self.list(source_cwd)?;
        if worktrees.len() <= keep {
            return Ok(PruneReport::default());
        }
        worktrees.sort_by(|left, right| {
            modified_at(&right.root)
                .cmp(&modified_at(&left.root))
                .then_with(|| left.root.cmp(&right.root))
        });

        let mut report = PruneReport::default();
        for checkout in worktrees.into_iter().skip(keep) {
            if let Some(owner) = self.owner(&checkout.root).unwrap_or(None) {
                if keep_owner(&owner) {
                    report.skipped.push(checkout.root);
                    continue;
                }
            }
            match self.remove(source_cwd, &checkout.root) {
                Ok(()) => report.removed.push(checkout.root),
                Err(error) => {
                    log::info!(
                        "worktree: prune skipped {}: {error:#}",
                        checkout.root.display()
                    );
                    report.skipped.push(checkout.root);
                }
            }
        }
        Ok(report)
    }

    /// Whether `path` is one of this app's managed checkouts.
    pub fn is_managed_path(&self, path: &Path) -> bool {
        let Ok(canonical) = fs::canonicalize(path) else {
            return false;
        };
        let managed_root = fs::canonicalize(&self.settings.root)
            .unwrap_or_else(|_| self.settings.root.clone());
        has_managed_layout(&managed_root, &canonical)
    }

    fn managed_checkout(&self, checkout: &Path) -> Result<PathBuf> {
        let canonical = fs::canonicalize(checkout)
            .with_context(|| format!("cannot resolve worktree {}", checkout.display()))?;
        if !self.is_managed_path(&canonical) {
            bail!("{} is not a managed worktree", checkout.display());
        }
        Ok(canonical)
    }
}

/// Describe any linked worktree without touching the app's managed state.
///
/// Returns `Ok(None)` for a main checkout, a non-repository path, or a path that
/// does not exist — callers use this to decide how to label a workspace.
pub fn inspect(cwd: &Path) -> Result<Option<WorktreeInfo>> {
    let managed_root = default_root().ok();
    inspect_with_root(cwd, managed_root.as_deref())
}

/// Same as [`inspect`], with an explicit managed root (tests, alternate profiles).
pub fn inspect_with_root(cwd: &Path, managed_root: Option<&Path>) -> Result<Option<WorktreeInfo>> {
    let Ok(canonical_cwd) = fs::canonicalize(cwd) else {
        return Ok(None);
    };
    let Ok(root) = repository_root(&canonical_cwd) else {
        return Ok(None);
    };
    let git_dir = resolve_git_path(&root, "--git-dir")?;
    let common_dir = resolve_git_path(&root, "--git-common-dir")?;
    if git_dir == common_dir {
        return Ok(None);
    }
    let head_sha = rev_parse_commit(&root, "HEAD")?;
    let branch = git::current_branch(&root);
    let source_root = common_dir
        .parent()
        .map(|parent| parent.to_path_buf())
        .unwrap_or_else(|| root.clone());
    let managed = managed_root
        .map(|managed_root| {
            let managed_root =
                fs::canonicalize(managed_root).unwrap_or_else(|_| managed_root.to_path_buf());
            has_managed_layout(&managed_root, &root)
        })
        .unwrap_or(false);
    Ok(Some(WorktreeInfo {
        root: simplify_path(&root),
        cwd: simplify_path(&canonical_cwd),
        source_root: simplify_path(&source_root),
        head_sha,
        detached: branch.is_none(),
        branch,
        managed,
    }))
}

fn remove_worktree(source_root: &Path, root: &Path) -> Result<()> {
    let result = git_output(
        source_root,
        GitOperation::WorkingTree,
        vec![
            OsStr::new("worktree"),
            OsStr::new("remove"),
            OsStr::new("--force"),
            root.as_os_str(),
        ],
    );
    remove_empty_bucket(root);
    result.map(|_| ())
}

fn is_safe_worktree_cwd(root: &Path, cwd: &Path) -> bool {
    let Ok(root) = fs::canonicalize(root) else {
        return false;
    };
    fs::canonicalize(cwd).is_ok_and(|resolved| resolved.is_dir() && resolved.starts_with(&root))
}

fn modified_at(path: &Path) -> std::time::SystemTime {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or(std::time::UNIX_EPOCH)
}

/// Parse `git worktree list --porcelain -z` into `(root, head, branch)` tuples.
fn parse_worktree_list(stdout: &[u8]) -> Vec<(PathBuf, Option<String>, Option<String>)> {
    let mut entries = Vec::new();
    let mut root: Option<PathBuf> = None;
    let mut head: Option<String> = None;
    let mut branch: Option<String> = None;

    for field in stdout.split(|byte| *byte == 0) {
        if field.is_empty() {
            if let Some(root) = root.take() {
                entries.push((root, head.take(), branch.take()));
            }
            head = None;
            branch = None;
            continue;
        }
        if let Some(path) = field.strip_prefix(b"worktree ") {
            root = git::path_from_bytes(path).ok();
        } else if let Some(value) = field.strip_prefix(b"HEAD ") {
            head = std::str::from_utf8(value).ok().map(str::to_owned);
        } else if let Some(value) = field.strip_prefix(b"branch ") {
            let name = value.strip_prefix(b"refs/heads/").unwrap_or(value);
            branch = std::str::from_utf8(name).ok().map(str::to_owned);
        }
    }
    if let Some(root) = root.take() {
        entries.push((root, head, branch));
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::test_support::{commit_file, git, init_repo};
    use super::*;

    fn manager(temp: &tempfile::TempDir) -> WorktreeManager {
        WorktreeManager::with_root(temp.path().join("managed"))
    }

    #[test]
    fn create_makes_a_detached_checkout_under_the_managed_root() {
        let (temp, repo) = init_repo();
        let manager = manager(&temp);
        let created = manager
            .create(&CreateWorktree {
                source_cwd: repo.clone(),
                base: None,
            })
            .unwrap();

        assert!(created.cwd.is_dir());
        assert!(created.root.join(".git").is_file());
        // Reported paths are canonical (macOS temp dirs resolve through /private).
        assert!(created
            .root
            .starts_with(fs::canonicalize(temp.path().join("managed")).unwrap()));
        assert_eq!(created.branch, None);
        // Detached HEAD: the new tree must not occupy a branch.
        assert_eq!(
            git(&created.root, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
            "HEAD"
        );
        assert_eq!(created.head_sha, git(&repo, &["rev-parse", "HEAD"]).trim());
        assert!(created.root.join("README.md").is_file());
    }

    #[test]
    fn create_defaults_to_the_current_head_not_the_default_branch() {
        let (temp, repo) = init_repo();
        commit_file(&repo, "second.txt", "second", "second commit");
        let head = git(&repo, &["rev-parse", "HEAD"]).trim().to_owned();

        let manager = manager(&temp);
        let created = manager
            .create(&CreateWorktree {
                source_cwd: repo,
                base: None,
            })
            .unwrap();
        assert_eq!(created.head_sha, head);
        assert!(created.root.join("second.txt").is_file());
    }

    #[test]
    fn create_mirrors_the_source_subdirectory() {
        let (temp, repo) = init_repo();
        let sub = repo.join("crates").join("app");
        fs::create_dir_all(&sub).unwrap();
        commit_file(&repo, "crates/app/main.rs", "fn main() {}", "add subdir");

        let manager = manager(&temp);
        let created = manager
            .create(&CreateWorktree {
                source_cwd: sub.clone(),
                base: None,
            })
            .unwrap();
        assert_eq!(created.cwd, created.root.join("crates").join("app"));
        assert!(created.cwd.is_dir());
    }

    #[test]
    fn create_rejects_a_non_repository_path() {
        let temp = tempfile::tempdir().unwrap();
        let plain = temp.path().join("plain");
        fs::create_dir_all(&plain).unwrap();
        let manager = manager(&temp);
        assert!(manager
            .create(&CreateWorktree {
                source_cwd: plain,
                base: None,
            })
            .is_err());
    }

    #[test]
    fn create_rejects_an_unknown_base() {
        let (temp, repo) = init_repo();
        let manager = manager(&temp);
        assert!(manager
            .create(&CreateWorktree {
                source_cwd: repo,
                base: Some("no-such-ref".to_owned()),
            })
            .is_err());
    }

    #[test]
    fn list_reports_only_managed_checkouts() {
        let (temp, repo) = init_repo();
        let manager = manager(&temp);
        let created = manager
            .create(&CreateWorktree {
                source_cwd: repo.clone(),
                base: None,
            })
            .unwrap();

        // A hand-made worktree outside the managed root must stay invisible.
        let manual = temp.path().join("manual");
        git(
            &repo,
            &["worktree", "add", "--detach", manual.to_str().unwrap(), "HEAD"],
        );

        let listed = manager.list(&repo).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].root, created.root);
        assert_eq!(listed[0].cwd, created.cwd);
    }

    #[test]
    fn remove_refuses_uncommitted_changes_then_succeeds_when_clean() {
        let (temp, repo) = init_repo();
        let manager = manager(&temp);
        let created = manager
            .create(&CreateWorktree {
                source_cwd: repo.clone(),
                base: None,
            })
            .unwrap();

        fs::write(created.root.join("README.md"), "dirty").unwrap();
        assert!(manager.remove(&repo, &created.root).is_err());

        git(&created.root, &["checkout", "--", "README.md"]);
        manager.remove(&repo, &created.root).unwrap();
        assert!(!created.root.exists());
    }

    #[test]
    fn remove_refuses_a_worktree_with_ignored_files() {
        let (temp, repo) = init_repo();
        let manager = manager(&temp);
        let created = manager
            .create(&CreateWorktree {
                source_cwd: repo.clone(),
                base: None,
            })
            .unwrap();

        fs::write(created.root.join(".gitignore"), "node_modules/\n").unwrap();
        fs::create_dir_all(created.root.join("node_modules")).unwrap();
        fs::write(created.root.join("node_modules/dep.js"), "x").unwrap();
        git(&created.root, &["add", ".gitignore"]);
        git(&created.root, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-m", "ignore"]);
        assert!(manager.remove(&repo, &created.root).is_err());
    }

    #[test]
    fn prune_keeps_the_newest_and_preserves_live_owners() {
        let (temp, repo) = init_repo();
        let manager = manager(&temp);
        let mut created = Vec::new();
        for index in 0..3 {
            let checkout = manager
                .create(&CreateWorktree {
                    source_cwd: repo.clone(),
                    base: None,
                })
                .unwrap();
            manager
                .bind_thread(&checkout.root, &format!("conv-{index}"))
                .unwrap();
            created.push(checkout);
            // Make mtimes strictly increasing so "newest" is deterministic.
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let oldest = created[0].root.clone();
        let middle = created[1].root.clone();
        let report = manager
            .prune(&repo, 1, |owner| owner == "conv-0")
            .unwrap();
        assert!(oldest.exists(), "live owner must survive pruning");
        assert!(report.skipped.contains(&oldest));
        assert!(!middle.exists(), "unowned-but-old checkout should be pruned");
    }

    #[test]
    fn inspect_detects_a_linked_worktree_and_ignores_the_main_checkout() {
        let (temp, repo) = init_repo();
        let manager = manager(&temp);
        let created = manager
            .create(&CreateWorktree {
                source_cwd: repo.clone(),
                base: None,
            })
            .unwrap();

        assert!(inspect(&repo).unwrap().is_none());
        let info = inspect_with_root(&created.cwd, Some(&temp.path().join("managed")))
            .unwrap()
            .unwrap();
        assert_eq!(info.root, created.root);
        assert!(info.detached);
        assert_eq!(info.head_sha, created.head_sha);
        assert!(info.managed);
        // Without the managed root the checkout is still recognized, just not ours.
        assert!(!inspect(&created.cwd).unwrap().unwrap().managed);
        assert_eq!(
            fs::canonicalize(&info.source_root).unwrap(),
            fs::canonicalize(&repo).unwrap()
        );
    }

    #[test]
    fn is_managed_path_matches_only_the_managed_root() {
        let (temp, repo) = init_repo();
        let manager = manager(&temp);
        let created = manager
            .create(&CreateWorktree {
                source_cwd: repo.clone(),
                base: None,
            })
            .unwrap();
        assert!(manager.is_managed_path(&created.root));
        assert!(!manager.is_managed_path(&repo));
    }
}
