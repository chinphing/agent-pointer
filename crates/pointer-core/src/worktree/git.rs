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

use crate::env::{host_env, Environment, ExecOptions, ExecOutput};
use anyhow::{bail, Context, Result};
use std::cell::RefCell;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

thread_local! {
    static GIT_ENV: RefCell<Option<Arc<dyn Environment>>> = const { RefCell::new(None) };
}

/// Run `body` with git and worktree path checks bound to `env`.
///
/// Local callers leave this unset and keep the host environment. Sandbox
/// creation sets the conversation environment so `git` runs inside it.
pub(crate) fn with_git_env<T>(env: Arc<dyn Environment>, body: impl FnOnce() -> T) -> T {
    struct Guard(Option<Arc<dyn Environment>>);
    impl Drop for Guard {
        fn drop(&mut self) {
            let prev = self.0.take();
            GIT_ENV.with(|slot| *slot.borrow_mut() = prev);
        }
    }
    let prev = GIT_ENV.with(|slot| slot.replace(Some(env)));
    let _guard = Guard(prev);
    body()
}

fn active_git_env() -> Arc<dyn Environment> {
    GIT_ENV
        .with(|slot| slot.borrow().clone())
        .unwrap_or_else(host_env)
}

pub(crate) fn git_on_host() -> bool {
    active_git_env().path_map().is_host_passthrough()
}

pub(crate) fn canonicalize_path(path: &Path) -> Result<PathBuf> {
    let env = active_git_env();
    if env.path_map().is_host_passthrough() {
        std::fs::canonicalize(path)
            .with_context(|| format!("cannot resolve {}", path.display()))
    } else {
        env.canonicalize(path).with_context(|| {
            format!(
                "cannot resolve {} in the session environment",
                path.display()
            )
        })
    }
}

pub(crate) fn canonicalize_path_or_original(path: &Path) -> PathBuf {
    match canonicalize_path(path) {
        Ok(path) => path,
        Err(error) => {
            log::warn!("worktree: canonicalize failed for {}: {error:#}", path.display());
            path.to_path_buf()
        }
    }
}

pub(crate) fn path_is_dir(path: &Path) -> bool {
    if git_on_host() {
        path.is_dir()
    } else {
        active_git_env()
            .metadata(path)
            .map(|meta| meta.is_dir)
            .unwrap_or(false)
    }
}

pub(crate) fn path_is_file(path: &Path) -> bool {
    if git_on_host() {
        std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file())
    } else {
        active_git_env()
            .metadata(path)
            .map(|meta| meta.is_file)
            .unwrap_or(false)
    }
}

pub(crate) fn ensure_dir_all(path: &Path) -> Result<()> {
    let env = active_git_env();
    if env.path_map().is_host_passthrough() {
        std::fs::create_dir_all(path)
            .with_context(|| format!("cannot create directory {}", path.display()))
    } else {
        env.create_dir_all(path).with_context(|| {
            format!(
                "cannot create directory {} in the session environment",
                path.display()
            )
        })
    }
}

/// Create `path` only when it does not exist. `Ok(true)` means this call created it.
pub(crate) fn create_dir_exclusive(path: &Path) -> Result<bool> {
    if git_on_host() {
        match std::fs::create_dir(path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(error)
                .with_context(|| format!("cannot create directory {}", path.display())),
        }
    } else {
        let env = active_git_env();
        if env.metadata(path).is_ok() {
            return Ok(false);
        }
        env.create_dir_all(path).with_context(|| {
            format!(
                "cannot create directory {} in the session environment",
                path.display()
            )
        })?;
        Ok(true)
    }
}

pub(crate) fn remove_empty_dir(path: &Path) {
    if git_on_host() {
        let _ = std::fs::remove_dir(path);
        return;
    }
    let env = active_git_env();
    match env.list_dir(path) {
        Ok(entries) if entries.is_empty() => {
            if let Err(error) = env.remove_dir_all(path) {
                log::warn!(
                    "worktree: failed to remove empty directory {}: {error:#}",
                    path.display()
                );
            }
        }
        Ok(_) => {}
        Err(error) => {
            log::warn!(
                "worktree: cannot inspect directory {} before removal: {error:#}",
                path.display()
            );
        }
    }
}

pub(crate) fn read_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    if git_on_host() {
        std::fs::read(path)
    } else {
        let env = active_git_env();
        if env.metadata(path).is_err() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("not found: {}", path.display()),
            ));
        }
        env.read_file(path)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, format!("{error:#}")))
    }
}

pub(crate) fn write_bytes(path: &Path, data: &[u8]) -> Result<()> {
    let env = active_git_env();
    if env.path_map().is_host_passthrough() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, data)
            .with_context(|| format!("cannot write {}", path.display()))
    } else {
        if let Some(parent) = path.parent() {
            env.create_dir_all(parent)?;
        }
        env.write_file(path, data)
            .with_context(|| format!("cannot write {} in the session environment", path.display()))
    }
}

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

pub(crate) fn git_output<I, S>(cwd: &Path, operation: GitOperation, args: I) -> Result<ExecOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut argv = git_argv();
    if operation == GitOperation::WorkingTree {
        for filter in configured_filters(cwd)? {
            argv.push("-c".to_string());
            argv.push(format!("filter.{filter}.process="));
            argv.push("-c".to_string());
            argv.push(format!("filter.{filter}.clean="));
            argv.push("-c".to_string());
            argv.push(format!("filter.{filter}.smudge="));
            argv.push("-c".to_string());
            argv.push(format!("filter.{filter}.required=false"));
        }
    }
    argv.extend(
        args.into_iter()
            .map(|a| a.as_ref().to_string_lossy().into_owned()),
    );
    // 本地档位走宿主 git。沙箱档位由 `with_git_env` 绑到会话环境，在容器里执行。
    let output = active_git_env()
        .exec(&argv, git_options(cwd))
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
    canonicalize_path(&raw)
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
    canonicalize_path(&raw)
        .with_context(|| format!("cannot resolve git path {}", raw.display()))
}

/// git 加固参数：禁止仓库配置执行用户代码（hooks / fsmonitor / attributes / filters）。
fn git_argv() -> Vec<String> {
    vec![
        "git".to_string(),
        "-c".to_string(),
        format!("core.hooksPath={DISABLED_HOOKS_PATH}"),
        "-c".to_string(),
        "core.fsmonitor=".to_string(),
        "-c".to_string(),
        "attr.tree=".to_string(),
        "-c".to_string(),
        "core.attributesFile=".to_string(),
    ]
}

/// 每次 git 调用都清掉会改写仓库选择的环境变量，并且不提示凭证、不 smudge LFS。
fn git_options(cwd: &Path) -> ExecOptions {
    let mut opts = ExecOptions::new()
        .with_cwd(cwd)
        .with_env("GIT_LFS_SKIP_SMUDGE", "1")
        .with_env("GIT_TERMINAL_PROMPT", "0");
    for name in SCRUBBED_ENV {
        opts = opts.without_env(*name);
    }
    opts
}

/// Drivers that configure a content filter; creation must not run them.
fn configured_filters(cwd: &Path) -> Result<Vec<String>> {
    let mut argv = git_argv();
    argv.extend(
        [
            "config",
            "--null",
            "--name-only",
            "--get-regexp",
            "^filter\\.",
        ]
        .into_iter()
        .map(str::to_string),
    );
    let output = active_git_env()
        .exec(&argv, git_options(cwd))
        .context("failed to inspect git filter configuration")?;
    if output.status != 0 {
        // Exit code 1 means "no matching config", which is the common case.
        if output.status == 1 {
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

fn ensure_success(output: &ExecOutput) -> Result<()> {
    if output.status == 0 {
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
