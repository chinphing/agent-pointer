use anyhow::{anyhow, Context, Result};
use crate::text_diff::compute_diff_lines;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEntry {
    pub name: String,
    pub path: String,
    pub kind: WorkspaceEntryKind,
    pub size_bytes: Option<u64>,
}

pub const WORKSPACE_FILE_PREVIEW_MAX_BYTES: usize = 1024 * 1024;

/// Max bytes per side when building a full-file Git review diff.
pub const GIT_FULL_DIFF_MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFilePreview {
    pub path: String,
    pub content: Option<String>,
    pub size_bytes: u64,
    pub truncated: bool,
    pub binary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceEntryKind {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitChange {
    pub path: String,
    pub status: String,
    pub staged: bool,
    pub original_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitDiff {
    pub path: String,
    /// `unstaged` (index→worktree), `staged` (HEAD→index), or `untracked`.
    pub mode: String,
    pub diff_lines: Vec<serde_json::Value>,
    pub diff_stats: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GitErrorCode {
    NotRepository,
    GitNotInstalled,
    CommandFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitErrorInfo {
    pub code: GitErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusResponse {
    pub changes: Vec<GitChange>,
    pub error: Option<GitErrorInfo>,
}

#[derive(Debug)]
struct GitCommandError(GitErrorInfo);

impl fmt::Display for GitCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0.message)
    }
}

impl std::error::Error for GitCommandError {}

/// List one directory level below `workspace_root`. Symlinks are returned as
/// leaf entries and are never followed, preventing traversal outside the root.
pub fn list_directory(
    workspace_root: &Path,
    relative_path: Option<&str>,
) -> Result<Vec<WorkspaceEntry>> {
    let root = canonical_workspace(workspace_root)?;
    let relative = safe_relative(relative_path.unwrap_or(""))?;
    let directory = root
        .join(&relative)
        .canonicalize()
        .with_context(|| format!("workspace directory does not exist: {}", relative.display()))?;
    if !directory.starts_with(&root) {
        return Err(anyhow!("path is outside workspaceRoot"));
    }
    if !directory.is_dir() {
        return Err(anyhow!("workspace path is not a directory"));
    }

    let mut entries = Vec::new();
    for item in fs::read_dir(&directory)? {
        let item = item?;
        let metadata = fs::symlink_metadata(item.path())?;
        let kind = if metadata.file_type().is_symlink() {
            WorkspaceEntryKind::Symlink
        } else if metadata.is_dir() {
            WorkspaceEntryKind::Directory
        } else {
            WorkspaceEntryKind::File
        };
        let item_relative = item
            .path()
            .strip_prefix(&root)?
            .to_string_lossy()
            .replace('\\', "/");
        entries.push(WorkspaceEntry {
            name: item.file_name().to_string_lossy().into_owned(),
            path: item_relative,
            size_bytes: metadata.is_file().then_some(metadata.len()),
            kind,
        });
    }
    entries.sort_by(|a, b| {
        entry_rank(&a.kind)
            .cmp(&entry_rank(&b.kind))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

const SEARCH_SKIP_DIR_NAMES: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    "__pycache__",
    ".venv",
    "venv",
    "vendor",
    ".turbo",
    ".cache",
];

/// Case-insensitive filename / relative-path search under the workspace root.
/// Skips heavy/vendor directories; returns at most `limit` entries (files, dirs, symlinks).
pub fn search_entries(
    workspace_root: &Path,
    query: &str,
    limit: usize,
) -> Result<Vec<WorkspaceEntry>> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let root = canonical_workspace(workspace_root)?;
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        if out.len() >= limit {
            break;
        }
        let read = match fs::read_dir(&dir) {
            Ok(iter) => iter,
            Err(err) => {
                log::warn!(
                    "workspace_read: search skip unreadable dir {}: {err}",
                    dir.display()
                );
                continue;
            }
        };
        for item in read {
            if out.len() >= limit {
                break;
            }
            let item = match item {
                Ok(v) => v,
                Err(err) => {
                    log::warn!("workspace_read: search skip entry: {err}");
                    continue;
                }
            };
            let metadata = match fs::symlink_metadata(item.path()) {
                Ok(m) => m,
                Err(err) => {
                    log::warn!(
                        "workspace_read: search skip metadata {}: {err}",
                        item.path().display()
                    );
                    continue;
                }
            };
            let name = item.file_name().to_string_lossy().into_owned();
            let kind = if metadata.file_type().is_symlink() {
                WorkspaceEntryKind::Symlink
            } else if metadata.is_dir() {
                WorkspaceEntryKind::Directory
            } else {
                WorkspaceEntryKind::File
            };
            let item_relative = match item.path().strip_prefix(&root) {
                Ok(p) => p.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            let name_l = name.to_lowercase();
            let path_l = item_relative.to_lowercase();
            if name_l.contains(&needle) || path_l.contains(&needle) {
                out.push(WorkspaceEntry {
                    name: name.clone(),
                    path: item_relative.clone(),
                    size_bytes: metadata.is_file().then_some(metadata.len()),
                    kind: kind.clone(),
                });
            }
            if matches!(kind, WorkspaceEntryKind::Directory)
                && !SEARCH_SKIP_DIR_NAMES
                    .iter()
                    .any(|skip| name.eq_ignore_ascii_case(skip))
            {
                stack.push(item.path());
            }
        }
    }
    out.sort_by(|a, b| {
        entry_rank(&a.kind)
            .cmp(&entry_rank(&b.kind))
            .then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase()))
    });
    if out.len() > limit {
        out.truncate(limit);
    }
    Ok(out)
}

/// Delete a file, symlink, or directory under `workspace_root`.
///
/// Safety: relative paths only (no `..` / absolute); never deletes the workspace
/// root itself; does not follow directory symlinks (`remove_file` for symlinks).
pub fn delete_path(workspace_root: &Path, relative_path: &str) -> Result<()> {
    let root = canonical_workspace(workspace_root)?;
    let relative = safe_relative(relative_path)?;
    if relative.as_os_str().is_empty() {
        return Err(anyhow!("cannot delete workspace root"));
    }
    let target = root.join(&relative);
    if !target.starts_with(&root) {
        return Err(anyhow!("path is outside workspaceRoot"));
    }
    let metadata = fs::symlink_metadata(&target).with_context(|| {
        format!("workspace path does not exist: {}", relative.display())
    })?;
    let file_type = metadata.file_type();
    let display = relative.to_string_lossy().replace('\\', "/");
    if file_type.is_symlink() || file_type.is_file() {
        fs::remove_file(&target)
            .with_context(|| format!("failed to delete file: {display}"))?;
        log::info!("workspace_read: deleted file path={display}");
    } else if file_type.is_dir() {
        fs::remove_dir_all(&target)
            .with_context(|| format!("failed to delete directory: {display}"))?;
        log::info!("workspace_read: deleted directory path={display}");
    } else {
        return Err(anyhow!("unsupported file type for delete: {display}"));
    }
    Ok(())
}

/// Read a bounded, UTF-8 text preview without following paths outside the workspace.
pub fn read_file(workspace_root: &Path, relative_path: &str) -> Result<WorkspaceFilePreview> {
    let root = canonical_workspace(workspace_root)?;
    let relative = safe_relative(relative_path)?;
    if relative.as_os_str().is_empty() {
        return Err(anyhow!("file path is required"));
    }
    let file_path = root
        .join(&relative)
        .canonicalize()
        .with_context(|| format!("workspace file does not exist: {}", relative.display()))?;
    if !file_path.starts_with(&root) {
        return Err(anyhow!("path is outside workspaceRoot"));
    }
    if !file_path.is_file() {
        return Err(anyhow!("workspace path is not a file"));
    }

    let size_bytes = fs::metadata(&file_path)?.len();
    let mut bytes = Vec::with_capacity(WORKSPACE_FILE_PREVIEW_MAX_BYTES + 1);
    fs::File::open(&file_path)?
        .take((WORKSPACE_FILE_PREVIEW_MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    let truncated = bytes.len() > WORKSPACE_FILE_PREVIEW_MAX_BYTES;
    bytes.truncate(WORKSPACE_FILE_PREVIEW_MAX_BYTES);
    let has_nul = bytes.contains(&0);
    let content = if has_nul {
        None
    } else {
        match std::str::from_utf8(&bytes) {
            Ok(_) => Some(String::from_utf8(bytes).expect("validated UTF-8 workspace preview")),
            Err(error) if truncated && error.error_len().is_none() => {
                bytes.truncate(error.valid_up_to());
                Some(String::from_utf8(bytes).expect("trimmed UTF-8 workspace preview"))
            }
            Err(_) => None,
        }
    };
    let binary = content.is_none();

    Ok(WorkspaceFilePreview {
        path: relative.to_string_lossy().replace('\\', "/"),
        content,
        size_bytes,
        truncated,
        binary,
    })
}

pub fn git_status(workspace_root: &Path) -> Result<Vec<GitChange>> {
    let root = canonical_workspace(workspace_root)?;
    let output = run_git(
        &root,
        ["status", "--porcelain=v2", "-z", "--untracked-files=all"],
    )?;
    Ok(parse_git_status(&output))
}

pub fn git_status_response(workspace_root: &Path) -> Result<GitStatusResponse> {
    match git_status(workspace_root) {
        Ok(changes) => Ok(GitStatusResponse {
            changes,
            error: None,
        }),
        Err(error) => match error.downcast_ref::<GitCommandError>() {
            Some(error) => Ok(GitStatusResponse {
                changes: Vec::new(),
                error: Some(error.0.clone()),
            }),
            None => Err(error),
        },
    }
}

fn parse_git_status(output: &[u8]) -> Vec<GitChange> {
    let fields: Vec<&[u8]> = output
        .split(|b| *b == 0)
        .filter(|field| !field.is_empty())
        .collect();
    let mut changes = Vec::new();
    let mut index = 0;
    while index < fields.len() {
        let record = String::from_utf8_lossy(fields[index]);
        if let Some(rest) = record.strip_prefix("1 ") {
            let parts: Vec<&str> = rest.splitn(8, ' ').collect();
            if parts.len() == 8 {
                changes.push(change(parts[7], parts[0], None));
            }
        } else if let Some(rest) = record.strip_prefix("2 ") {
            let parts: Vec<&str> = rest.splitn(9, ' ').collect();
            if parts.len() == 9 {
                let original = fields
                    .get(index + 1)
                    .map(|p| String::from_utf8_lossy(p).into_owned());
                changes.push(change(parts[8], parts[0], original));
                index += usize::from(original_field_present(fields.get(index + 1)));
            }
        } else if let Some(path) = record.strip_prefix("? ") {
            changes.push(change(path, "??", None));
        } else if let Some(path) = record
            .strip_prefix("u ")
            .and_then(|rest| rest.splitn(10, ' ').nth(9))
        {
            changes.push(change(path, "UU", None));
        }
        index += 1;
    }
    changes
}

/// Full-file Git review diff for one path.
///
/// - Untracked / unstaged worktree changes → index (or empty) vs disk  
/// - Staged-only → HEAD vs index  
pub fn git_diff(
    workspace_root: &Path,
    relative_path: &str,
    status: Option<&str>,
) -> Result<GitDiff> {
    let root = canonical_workspace(workspace_root)?;
    let relative = safe_relative(relative_path)?;
    if relative.as_os_str().is_empty() {
        return Err(anyhow!("file path is required"));
    }
    let path = relative.to_string_lossy().replace('\\', "/");
    let status = match status.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => resolve_git_status_for_path(&root, &path)?,
    };
    let mode = git_diff_mode(&status);
    let (before, after) = match mode {
        "untracked" => (String::new(), read_worktree_text(&root, &path)?),
        "unstaged" => (
            read_git_blob_text(&root, &format!(":{path}"))?.unwrap_or_default(),
            read_worktree_text(&root, &path)?,
        ),
        "staged" => (
            read_git_blob_text(&root, &format!("HEAD:{path}"))?.unwrap_or_default(),
            read_git_blob_text(&root, &format!(":{path}"))?.unwrap_or_default(),
        ),
        other => return Err(anyhow!("unsupported git diff mode: {other}")),
    };
    let (diff_lines, diff_stats) = compute_diff_lines(&before, &after);
    log::info!("workspace git full diff path={path} mode={mode}");
    Ok(GitDiff {
        path,
        mode: mode.to_string(),
        diff_lines,
        diff_stats,
    })
}

fn git_diff_mode(status: &str) -> &'static str {
    if status == "??" || status.starts_with('?') {
        return "untracked";
    }
    let bytes = status.as_bytes();
    let unstaged = bytes.get(1).is_some_and(|c| *c != b'.');
    if unstaged {
        return "unstaged";
    }
    let staged = bytes
        .first()
        .is_some_and(|c| *c != b'.' && *c != b'?');
    if staged {
        return "staged";
    }
    // Fallback: treat as unstaged worktree review.
    "unstaged"
}

fn resolve_git_status_for_path(root: &Path, path: &str) -> Result<String> {
    let changes = git_status(root)?;
    changes
        .into_iter()
        .find(|c| c.path == path)
        .map(|c| c.status)
        .ok_or_else(|| anyhow!("path not in git status: {path}"))
}

fn read_git_blob_text(root: &Path, spec: &str) -> Result<Option<String>> {
    match run_git(root, ["show", spec]) {
        Ok(bytes) => Ok(Some(decode_text_blob(&bytes, spec)?)),
        Err(error) => {
            if error.downcast_ref::<GitCommandError>().is_some() {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}

fn read_worktree_text(root: &Path, relative: &str) -> Result<String> {
    let full = root.join(relative);
    if !full.exists() {
        return Ok(String::new());
    }
    let meta = fs::metadata(&full).with_context(|| format!("stat {}", full.display()))?;
    if !meta.is_file() {
        return Err(anyhow!("不是常规文件: {}", full.display()));
    }
    if meta.len() as usize > GIT_FULL_DIFF_MAX_BYTES {
        return Err(anyhow!(
            "文件过大（>{} bytes），无法展示整文件 Diff",
            GIT_FULL_DIFF_MAX_BYTES
        ));
    }
    let bytes = fs::read(&full).with_context(|| format!("read {}", full.display()))?;
    decode_text_blob(&bytes, relative)
}

fn decode_text_blob(bytes: &[u8], label: &str) -> Result<String> {
    if bytes.len() > GIT_FULL_DIFF_MAX_BYTES {
        return Err(anyhow!(
            "内容过大（>{} bytes）：{label}",
            GIT_FULL_DIFF_MAX_BYTES
        ));
    }
    if bytes.contains(&0) {
        return Err(anyhow!("二进制文件不支持 Diff：{label}"));
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| anyhow!("非 UTF-8 文本，无法 Diff：{label}"))
}

fn original_field_present(field: Option<&&[u8]>) -> bool {
    field.is_some()
}

fn change(path: &str, status: &str, original_path: Option<String>) -> GitChange {
    GitChange {
        path: path.replace('\\', "/"),
        status: status.to_owned(),
        staged: status
            .as_bytes()
            .first()
            .is_some_and(|c| *c != b'.' && *c != b'?'),
        original_path,
    }
}

fn canonical_workspace(workspace_root: &Path) -> Result<PathBuf> {
    let root = workspace_root
        .canonicalize()
        .context("workspaceRoot does not exist")?;
    if !root.is_dir() {
        return Err(anyhow!("workspaceRoot is not a directory"));
    }
    Ok(root)
}

fn safe_relative(value: &str) -> Result<PathBuf> {
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(anyhow!("path must be relative to workspaceRoot"));
    }
    Ok(path.to_path_buf())
}

fn run_git<I, S>(root: &Path, args: I) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(git_spawn_error)?;
    parse_git_output(output)
}

fn git_spawn_error(error: io::Error) -> anyhow::Error {
    let info = if error.kind() == io::ErrorKind::NotFound {
        GitErrorInfo {
            code: GitErrorCode::GitNotInstalled,
            message: "Git 未安装或不可用".to_owned(),
        }
    } else {
        GitErrorInfo {
            code: GitErrorCode::CommandFailed,
            message: format!("无法运行 Git：{error}"),
        }
    };
    GitCommandError(info).into()
}

fn parse_git_output(output: Output) -> Result<Vec<u8>> {
    if output.status.success() {
        return Ok(output.stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let code = if stderr.contains("not a git repository") {
        GitErrorCode::NotRepository
    } else {
        GitErrorCode::CommandFailed
    };
    Err(GitCommandError(GitErrorInfo {
        code,
        message: if stderr.is_empty() {
            "Git 命令执行失败".to_owned()
        } else {
            stderr
        },
    })
    .into())
}

fn entry_rank(kind: &WorkspaceEntryKind) -> u8 {
    match kind {
        WorkspaceEntryKind::Directory => 0,
        WorkspaceEntryKind::File => 1,
        WorkspaceEntryKind::Symlink => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rejects_paths_outside_workspace() {
        let root = tempfile::tempdir().unwrap();
        assert!(list_directory(root.path(), Some("../outside")).is_err());
        assert!(git_diff(root.path(), "/tmp/outside", None).is_err());
        assert!(delete_path(root.path(), "../outside").is_err());
        assert!(delete_path(root.path(), "").is_err());
    }

    #[test]
    fn deletes_files_and_directories_inside_workspace() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("folder")).unwrap();
        fs::write(root.path().join("folder/nested.txt"), b"nested").unwrap();
        fs::write(root.path().join("file.txt"), b"hello").unwrap();

        delete_path(root.path(), "file.txt").unwrap();
        assert!(!root.path().join("file.txt").exists());

        delete_path(root.path(), "folder").unwrap();
        assert!(!root.path().join("folder").exists());
        assert!(delete_path(root.path(), "missing.txt").is_err());
    }

    #[test]
    fn search_entries_matches_nested_names_and_skips_node_modules() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/App.vue"), b"app").unwrap();
        fs::create_dir(root.path().join("node_modules")).unwrap();
        fs::write(root.path().join("node_modules/hidden.ts"), b"x").unwrap();

        let hits = search_entries(root.path(), "app", 50).unwrap();
        assert!(hits.iter().any(|e| e.path == "src/App.vue"));
        assert!(!hits.iter().any(|e| e.path.contains("node_modules")));
    }

    #[test]
    fn lists_only_one_level_and_does_not_follow_symlinks() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("folder")).unwrap();
        fs::write(root.path().join("file.txt"), b"hello").unwrap();
        fs::write(root.path().join("folder/nested.txt"), b"nested").unwrap();
        let entries = list_directory(root.path(), None).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, WorkspaceEntryKind::Directory);
        assert_eq!(entries[1].size_bytes, Some(5));
    }

    #[test]
    fn reads_text_preview_and_rejects_binary_or_escaped_paths() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("file.txt"), "hello\n世界\n").unwrap();
        fs::write(root.path().join("binary.bin"), b"abc\0def").unwrap();

        let text = read_file(root.path(), "file.txt").unwrap();
        assert_eq!(text.content.as_deref(), Some("hello\n世界\n"));
        assert!(!text.binary);
        assert!(!text.truncated);

        let binary = read_file(root.path(), "binary.bin").unwrap();
        assert!(binary.binary);
        assert!(binary.content.is_none());
        assert!(read_file(root.path(), "../outside.txt").is_err());
    }

    #[test]
    fn truncates_large_text_preview_on_a_utf8_boundary() {
        let root = tempfile::tempdir().unwrap();
        let content = format!("{}界", "a".repeat(WORKSPACE_FILE_PREVIEW_MAX_BYTES - 1));
        fs::write(root.path().join("large.txt"), content).unwrap();

        let preview = read_file(root.path(), "large.txt").unwrap();
        assert!(preview.truncated);
        assert!(!preview.binary);
        assert_eq!(
            preview.content.as_ref().unwrap().len(),
            WORKSPACE_FILE_PREVIEW_MAX_BYTES - 1
        );
    }

    #[test]
    fn classifies_git_command_failures_without_matching_in_the_frontend() {
        let output = Command::new("git")
            .args(["-C", "/definitely/not/a/workspace", "status"])
            .output()
            .unwrap();
        let error = parse_git_output(output).unwrap_err();
        assert_eq!(
            error.downcast_ref::<GitCommandError>().unwrap().0.code,
            GitErrorCode::CommandFailed
        );

        let missing = git_spawn_error(io::Error::new(io::ErrorKind::NotFound, "missing"));
        assert_eq!(
            missing.downcast_ref::<GitCommandError>().unwrap().0.code,
            GitErrorCode::GitNotInstalled
        );
    }

    #[test]
    fn reports_status_and_both_staged_and_unstaged_diff() {
        let root = tempfile::tempdir().unwrap();
        run_git(root.path(), ["init"]).unwrap();
        run_git(root.path(), ["config", "user.email", "test@example.com"]).unwrap();
        run_git(root.path(), ["config", "user.name", "Test"]).unwrap();
        fs::write(root.path().join("a.txt"), "one\n").unwrap();
        run_git(root.path(), ["add", "a.txt"]).unwrap();
        run_git(root.path(), ["commit", "-m", "initial"]).unwrap();
        fs::write(root.path().join("a.txt"), "two\n").unwrap();
        run_git(root.path(), ["add", "a.txt"]).unwrap();
        fs::write(root.path().join("a.txt"), "three\n").unwrap();

        let status = git_status(root.path()).unwrap();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].status, "MM");
        assert!(status[0].staged);

        // MM prefers unstaged: index ("two") → worktree ("three")
        let unstaged = git_diff(root.path(), "a.txt", Some("MM")).unwrap();
        assert_eq!(unstaged.mode, "unstaged");
        assert_eq!(unstaged.diff_stats["adds"].as_u64().unwrap(), 1);
        assert_eq!(unstaged.diff_stats["dels"].as_u64().unwrap(), 1);
        assert!(unstaged
            .diff_lines
            .iter()
            .any(|line| line["type"] == "del" && line["text"] == "two"));
        assert!(unstaged
            .diff_lines
            .iter()
            .any(|line| line["type"] == "ins" && line["text"] == "three"));

        // Staged-only uses HEAD → index
        let staged = git_diff(root.path(), "a.txt", Some("M.")).unwrap();
        assert_eq!(staged.mode, "staged");
        assert!(staged
            .diff_lines
            .iter()
            .any(|line| line["type"] == "del" && line["text"] == "one"));
        assert!(staged
            .diff_lines
            .iter()
            .any(|line| line["type"] == "ins" && line["text"] == "two"));
    }
}
