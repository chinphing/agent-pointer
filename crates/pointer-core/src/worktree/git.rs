//! Isolated git invocations for managed worktrees.
//!
//! Every command:
//! - drops inherited repository selectors (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, …)
//!   so a git wrapper or hook that exported them cannot redirect us to another repo;
//! - disables repository-configured hooks, fsmonitor, attributes and clean/smudge/process
//!   filters, so creating a worktree never executes user code;
//! - never prompts for credentials and skips LFS smudging.
//!
//! This mirrors the hardening OpenAI Codex applies in `codex-rs/worktree/src/git.rs`.

use anyhow::{bail, Context, Result};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const DISABLED_HOOKS_PATH: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

/// Environment variables that would override the repository we select via `-C`.
const SCRUBBED_ENV: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CEILING_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum GitOperation {
    /// Read-only repository metadata.
    Metadata,
    /// Touches the working tree (also disables content filters).
    WorkingTree,
}

pub(crate) fn git_output<I, S>(cwd: &Path, operation: GitOperation, args: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = base_command(cwd);
    if operation == GitOperation::WorkingTree {
        for filter in configured_filters(cwd)? {
            command
                .arg("-c")
                .arg(format!("filter.{filter}.process="))
                .arg("-c")
                .arg(format!("filter.{filter}.clean="))
                .arg("-c")
                .arg(format!("filter.{filter}.smudge="))
                .arg("-c")
                .arg(format!("filter.{filter}.required=false"));
        }
    }
    let output = command
        .args(args)
        .output()
        .context("failed to start git (is git installed?)")?;
    ensure_success(&output)?;
    Ok(output)
}

pub(crate) fn git_stdout<I, S>(cwd: &Path, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_output(cwd, GitOperation::WorkingTree, args)?;
    let stdout = String::from_utf8(output.stdout).context("git output is not valid UTF-8")?;
    Ok(stdout.trim().to_owned())
}

/// Run a metadata command that prints a single filesystem path.
pub(crate) fn git_path<I, S>(cwd: &Path, args: I) -> Result<PathBuf>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_output(cwd, GitOperation::Metadata, args)?;
    let bytes = output
        .stdout
        .strip_suffix(b"\n")
        .unwrap_or(&output.stdout);
    #[cfg(windows)]
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    path_from_bytes(bytes)
}

pub(crate) fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf> {
    if bytes.is_empty() {
        bail!("git returned an empty path");
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec())))
    }
    #[cfg(not(unix))]
    {
        Ok(PathBuf::from(
            std::str::from_utf8(bytes).context("git path is not valid UTF-8")?,
        ))
    }
}

/// Repository root that contains `cwd`, canonicalized.
pub(crate) fn repository_root(cwd: &Path) -> Result<PathBuf> {
    let raw = git_path(cwd, ["rev-parse", "--show-toplevel"])
        .with_context(|| format!("{} is not inside a git repository", cwd.display()))?;
    std::fs::canonicalize(&raw)
        .with_context(|| format!("cannot resolve repository root {}", raw.display()))
}

/// Commit id for `revision` (already peeled to a commit).
pub(crate) fn rev_parse_commit(cwd: &Path, revision: &str) -> Result<String> {
    let spec = format!("{revision}^{{commit}}");
    let sha = git_stdout(
        cwd,
        ["rev-parse", "--verify", "--end-of-options", spec.as_str()],
    )?;
    if sha.is_empty() {
        bail!("git returned an empty commit id for {revision}");
    }
    Ok(sha)
}

/// Current branch name, or `None` for a detached HEAD.
pub(crate) fn current_branch(cwd: &Path) -> Option<String> {
    let name = git_stdout(cwd, ["rev-parse", "--abbrev-ref", "HEAD"]).ok()?;
    if name.is_empty() || name == "HEAD" {
        None
    } else {
        Some(name)
    }
}

/// Resolve a git path (`--git-dir`, `--git-common-dir`, `--git-path <x>`) to an absolute path.
///
/// `--path-format=absolute` needs git 2.31+; older git falls back to joining the
/// (possibly relative) result against `cwd`.
pub(crate) fn resolve_git_path(cwd: &Path, argument: &str) -> Result<PathBuf> {
    let raw = match git_path(cwd, ["rev-parse", "--path-format=absolute", argument]) {
        Ok(path) => path,
        Err(_) => {
            let relative = git_path(cwd, ["rev-parse", argument])?;
            if relative.is_absolute() {
                relative
            } else {
                cwd.join(relative)
            }
        }
    };
    std::fs::canonicalize(&raw)
        .with_context(|| format!("cannot resolve git path {}", raw.display()))
}

fn base_command(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    for name in SCRUBBED_ENV {
        command.env_remove(name);
    }
    command
        .current_dir(cwd)
        .arg("-c")
        .arg("core.hooksPath=".to_owned() + DISABLED_HOOKS_PATH)
        .arg("-c")
        .arg("core.fsmonitor=")
        .arg("-c")
        .arg("attr.tree=")
        .arg("-c")
        .arg("core.attributesFile=")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Drivers that configure a content filter; creation must not run them.
fn configured_filters(cwd: &Path) -> Result<Vec<String>> {
    let output = base_command(cwd)
        .args([
            "config",
            "--null",
            "--name-only",
            "--get-regexp",
            "^filter\\.",
        ])
        .output()
        .context("failed to inspect git filter configuration")?;
    if !output.status.success() {
        // Exit code 1 means "no matching config", which is the common case.
        if output.status.code() == Some(1) {
            return Ok(Vec::new());
        }
        ensure_success(&output)?;
    }
    let names = String::from_utf8_lossy(&output.stdout);
    let mut drivers: Vec<String> = Vec::new();
    for name in names.split('\0') {
        let Some(rest) = name.strip_prefix("filter.") else {
            continue;
        };
        let Some((driver, attribute)) = rest.rsplit_once('.') else {
            continue;
        };
        if driver.is_empty()
            || !matches!(attribute, "clean" | "smudge" | "process" | "required")
            || drivers.iter().any(|existing| existing == driver)
        {
            continue;
        }
        drivers.push(driver.to_owned());
    }
    Ok(drivers)
}

fn ensure_success(output: &Output) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if stderr.is_empty() {
        bail!("git command failed with status {}", output.status);
    }
    bail!("git command failed: {stderr}");
}

/// Strip the Windows verbatim prefix so stored paths stay readable.
#[cfg(windows)]
pub(crate) fn simplify_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest.to_string());
    }
    path.to_path_buf()
}

#[cfg(not(windows))]
pub(crate) fn simplify_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}
