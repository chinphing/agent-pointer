use anyhow::{anyhow, Context, Result};
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitDiff {
    pub path: String,
    pub diff: String,
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

pub fn git_diff(workspace_root: &Path, relative_path: &str) -> Result<GitDiff> {
    let root = canonical_workspace(workspace_root)?;
    let relative = safe_relative(relative_path)?;
    if relative.as_os_str().is_empty() {
        return Err(anyhow!("file path is required"));
    }
    let path = relative.to_string_lossy().replace('\\', "/");
    let unstaged = run_git(&root, ["diff", "--no-ext-diff", "--", &path])?;
    let staged = run_git(&root, ["diff", "--cached", "--no-ext-diff", "--", &path])?;
    let mut diff = String::from_utf8_lossy(&staged).into_owned();
    diff.push_str(&String::from_utf8_lossy(&unstaged));
    Ok(GitDiff { path, diff })
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
        assert!(git_diff(root.path(), "/tmp/outside").is_err());
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
        let diff = git_diff(root.path(), "a.txt").unwrap().diff;
        assert!(diff.contains("-one"));
        assert!(diff.contains("+two"));
        assert!(diff.contains("-two"));
        assert!(diff.contains("+three"));
    }
}
