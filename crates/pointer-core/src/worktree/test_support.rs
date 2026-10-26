//! Test fixtures for managed-worktree tests.
//!
//! Every fixture is a real git repository created in a temp directory: the
//! behaviour under test is git's, so stubs would prove nothing.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run git with a throwaway identity and assert success.
pub(crate) fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
        .arg("-c")
        .arg("user.name=pointer-test")
        .arg("-c")
        .arg("user.email=pointer-test@example.com")
        .arg("-c")
        .arg("commit.gpgsign=false")
        .args(args)
        .output()
        .expect("git must be installed to run worktree tests");
    assert!(
        output.status.success(),
        "git {:?} failed in {}: {}",
        args,
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Fresh repository with one commit on `main`.
pub(crate) fn init_repo() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("temp dir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["-c", "init.defaultBranch=main", "init"]);
    fs::write(repo.join("README.md"), "hello\n").expect("readme");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "initial commit"]);
    (temp, repo)
}

/// Write `content` to `relative` inside `repo` and commit it.
pub(crate) fn commit_file(repo: &Path, relative: &str, content: &str, message: &str) {
    let target = repo.join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).expect("parent dir");
    }
    fs::write(&target, content).expect("write file");
    git(repo, &["add", relative]);
    git(repo, &["commit", "-m", message]);
}
