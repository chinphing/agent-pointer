use crate::text_diff::compute_diff_lines;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// 子进程不创建控制台窗口（避免 Windows 上 git 命令闪现黑框 / 拖慢工作区面板）。
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
/// 面板要操作的根目录。
///
/// headless/desktop 档位下工作区在沙箱里（`/workspace`），宿主路径在沙箱中不存在，
/// 所以一律以环境绑定的工作区为准；local 档位沿用调用方给的宿主路径。
fn panel_root(env: &dyn crate::env::Environment, workspace_root: &Path) -> Result<PathBuf> {
    if env.kind() != crate::env::EnvKind::Local {
        return Ok(env.workspace_root().to_path_buf());
    }
    canonical_workspace(workspace_root)
}

/// `env` 必须由调用方按会话解析后传入（Web 路由 / Tauri 命令用
/// [`crate::env::session_env::bind_workspace_request`]）。**这里不再自己取
/// `effective_env()`**：面板请求不是对话回合，线程上没有会话绑定，取到的会是
/// `DeniedEnv`——沙箱档位部署下连 admin 的面板都会全挂。
pub fn list_directory(
    env: &dyn crate::env::Environment,
    workspace_root: &Path,
    relative_path: Option<&str>,
) -> Result<Vec<WorkspaceEntry>> {
    let root = panel_root(env, workspace_root)?;
    let relative = safe_relative(relative_path.unwrap_or(""))?;
    let joined = root.join(&relative);
    let directory = env
        .canonicalize(&joined)
        .with_context(|| format!("workspace directory does not exist: {}", relative.display()))?;
    if !directory.starts_with(&root) {
        return Err(anyhow!("path is outside workspaceRoot"));
    }
    let dir_meta = env.metadata(&directory)?;
    if !dir_meta.is_dir {
        return Err(anyhow!("workspace path is not a directory"));
    }

    let mut entries = Vec::new();
    for item in env.list_dir(&directory)? {
        let metadata = env.metadata(&item.path).ok();
        let kind = if item.is_symlink {
            WorkspaceEntryKind::Symlink
        } else if item.is_dir {
            WorkspaceEntryKind::Directory
        } else {
            WorkspaceEntryKind::File
        };
        let item_relative = item
            .path
            .strip_prefix(&root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| item.name.clone());
        entries.push(WorkspaceEntry {
            name: item.name.clone(),
            path: item_relative,
            size_bytes: metadata.as_ref().filter(|m| m.is_file).map(|m| m.len),
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
///
/// `env` 由调用方按会话解析后传入（同 [`list_directory`]）。
pub fn search_entries(
    env: &dyn crate::env::Environment,
    workspace_root: &Path,
    query: &str,
    limit: usize,
) -> Result<Vec<WorkspaceEntry>> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let root = panel_root(env, workspace_root)?;
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        if out.len() >= limit {
            break;
        }
        let read = match env.list_dir(&dir) {
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
            let metadata = match env.metadata(&item.path) {
                Ok(m) => m,
                Err(err) => {
                    log::warn!(
                        "workspace_read: search skip metadata {}: {err}",
                        item.path.display()
                    );
                    continue;
                }
            };
            let name = item.name.clone();
            let kind = if item.is_symlink {
                WorkspaceEntryKind::Symlink
            } else if item.is_dir {
                WorkspaceEntryKind::Directory
            } else {
                WorkspaceEntryKind::File
            };
            let item_relative = match item.path.strip_prefix(&root) {
                Ok(p) => p.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            let name_l = name.to_lowercase();
            let path_l = item_relative.to_lowercase();
            if name_l.contains(&needle) || path_l.contains(&needle) {
                out.push(WorkspaceEntry {
                    name: name.clone(),
                    path: item_relative.clone(),
                    size_bytes: metadata.is_file.then_some(metadata.len),
                    kind: kind.clone(),
                });
            }
            if kind == WorkspaceEntryKind::Directory
                && !SEARCH_SKIP_DIR_NAMES.contains(&name.as_str())
                && !item.is_symlink
            {
                stack.push(item.path);
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
///
/// `env` 由调用方按会话解析后传入（同 [`list_directory`]）。
pub fn delete_path(
    env: &dyn crate::env::Environment,
    workspace_root: &Path,
    relative_path: &str,
) -> Result<()> {
    let root = panel_root(env, workspace_root)?;
    let relative = safe_relative(relative_path)?;
    if relative.as_os_str().is_empty() {
        return Err(anyhow!("cannot delete workspace root"));
    }
    let target = root.join(&relative);
    if !target.starts_with(&root) {
        return Err(anyhow!("path is outside workspaceRoot"));
    }
    let metadata = env
        .metadata(&target)
        .with_context(|| format!("workspace path does not exist: {}", relative.display()))?;
    let display = relative.to_string_lossy().replace('\\', "/");
    if metadata.is_symlink || metadata.is_file {
        env.remove_file(&target)
            .with_context(|| format!("failed to delete file: {display}"))?;
        log::info!("workspace_read: deleted file path={display}");
    } else if metadata.is_dir {
        env.remove_dir_all(&target)
            .with_context(|| format!("failed to delete directory: {display}"))?;
        log::info!("workspace_read: deleted directory path={display}");
    } else {
        return Err(anyhow!("unsupported file type for delete: {display}"));
    }
    Ok(())
}

/// Absolute path + MIME for streaming a workspace file (images/PDF inline preview).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMediaFile {
    pub path: PathBuf,
    pub mime_type: String,
    pub file_name: String,
}

/// Resolve a workspace-relative file for raw byte streaming (same path rules as [`read_file`]).
///
/// `env` 由调用方按会话解析后传入（同 [`list_directory`]）。
pub fn resolve_file_for_media(
    env: &dyn crate::env::Environment,
    workspace_root: &Path,
    relative_path: &str,
) -> Result<WorkspaceMediaFile> {
    let file_path = resolve_workspace_file_path_in(env, workspace_root, relative_path)?;
    let file_name = file_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("file")
        .to_owned();
    let mime_type = mime_from_path(&file_path);
    log::info!(
        "workspace_read: resolve media path={} mime={}",
        file_path.display(),
        mime_type
    );
    Ok(WorkspaceMediaFile {
        path: file_path,
        mime_type,
        file_name,
    })
}

/// 工作区媒体「整段读进内存」的上限（32 MB）。
///
/// 只有宿主上没有文件的远端后端（envd 云端等）才会走这条路；本地 / 容器档位
/// 直接流式读宿主文件，不占内存。超过上限就明确报错，让用户去下载，
/// **不静默截断**——截断的图片 / 视频在预览里是坏文件，比报错更难排查。
pub const WORKSPACE_MEDIA_INLINE_MAX_BYTES: u64 = 32 * 1024 * 1024;

/// 工作区媒体文件的字节来源。
///
/// 沙箱档位下 [`resolve_file_for_media`] 返回的是**环境内路径**（`/data/...`），
/// 字节一律经 [`Environment`] 取回；只有 local 恒等映射才直接打开宿主路径流式返回。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceMediaBody {
    /// local 档位：宿主就是环境，可直接流式打开。
    HostPath(PathBuf),
    /// 沙箱 / 远端：经环境读入内存。
    Inline(Vec<u8>),
}

impl WorkspaceMediaBody {
    /// 日志用：不泄露内容，只标来源。
    pub fn source_label(&self) -> &'static str {
        match self {
            Self::HostPath(_) => "host-stream",
            Self::Inline(_) => "env-inline",
        }
    }
}

/// 把已解析的工作区媒体路径落到「宿主流式」或「环境读内存」。
///
/// 1. 路径映射恒等（local）→ 宿主路径流式；
/// 2. 其余档位 → 只走 `env.metadata` / `env.read_file_capped`，不经 `to_host`。
pub fn media_body_for_streaming(
    env: &dyn crate::env::Environment,
    path: &Path,
    max_bytes: u64,
) -> Result<WorkspaceMediaBody> {
    if !env.is_denied() && env.path_map().is_host_passthrough() {
        return Ok(WorkspaceMediaBody::HostPath(path.to_path_buf()));
    }
    let meta = env
        .metadata(path)
        .with_context(|| format!("workspace file does not exist: {}", path.display()))?;
    if !meta.is_file {
        return Err(anyhow!("workspace path is not a file: {}", path.display()));
    }
    if meta.len > max_bytes {
        return Err(anyhow!(
            "工作区文件 {} 有 {} 字节，超过在线预览上限 {} 字节；请下载后查看",
            path.display(),
            meta.len,
            max_bytes
        ));
    }
    let bytes = env.read_file_capped(path, max_bytes)?;
    Ok(WorkspaceMediaBody::Inline(bytes))
}

fn resolve_workspace_file_path_in(
    env: &dyn crate::env::Environment,
    workspace_root: &Path,
    relative_path: &str,
) -> Result<PathBuf> {
    let root = panel_root(env, workspace_root)?;
    let relative = safe_relative(relative_path)?;
    if relative.as_os_str().is_empty() {
        return Err(anyhow!("file path is required"));
    }
    let file_path = env
        .canonicalize(&root.join(&relative))
        .with_context(|| format!("workspace file does not exist: {}", relative.display()))?;
    if !file_path.starts_with(&root) {
        return Err(anyhow!("path is outside workspaceRoot"));
    }
    if !env.metadata(&file_path)?.is_file {
        return Err(anyhow!("workspace path is not a file"));
    }
    Ok(file_path)
}

fn mime_from_path(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png".into(),
        Some("jpg") | Some("jpeg") => "image/jpeg".into(),
        Some("gif") => "image/gif".into(),
        Some("webp") => "image/webp".into(),
        Some("svg") => "image/svg+xml".into(),
        Some("bmp") => "image/bmp".into(),
        Some("ico") => "image/x-icon".into(),
        Some("pdf") => "application/pdf".into(),
        _ => "application/octet-stream".into(),
    }
}

/// Read a bounded, UTF-8 text preview without following paths outside the workspace.
///
/// `env` 由调用方按会话解析后传入（同 [`list_directory`]）。
pub fn read_file(
    env: &dyn crate::env::Environment,
    workspace_root: &Path,
    relative_path: &str,
) -> Result<WorkspaceFilePreview> {
    let file_path = resolve_workspace_file_path_in(env, workspace_root, relative_path)?;
    let relative = safe_relative(relative_path)?;

    let size_bytes = env.metadata(&file_path)?.len;
    // 预览只读前 N+1 字节：用流式读取，避免大文件整读进内存。
    let mut bytes = Vec::with_capacity(WORKSPACE_FILE_PREVIEW_MAX_BYTES + 1);
    env.open_reader(&file_path)?
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

/// `env` 由调用方按会话解析后传入（同 [`list_directory`]）。
pub fn git_status(
    env: &dyn crate::env::Environment,
    workspace_root: &Path,
) -> Result<Vec<GitChange>> {
    let root = panel_root(env, workspace_root)?;
    let output = run_git(
        env,
        &root,
        ["status", "--porcelain=v2", "-z", "--untracked-files=all"],
    )?;
    Ok(parse_git_status(&output))
}

/// Short-lived cache for `git status` results keyed by **host** workspace root.
///
/// Spawning `git` is expensive under real-time AV scanning on Windows (the
/// panel refreshes status on open and on every entry to Changes); the TTL
/// avoids re-spawning within a few seconds of the last run. Errors are not
/// cached so a transient failure retries immediately.
///
/// 只缓存 `local` 档位：沙箱档位下每个会话的 `workspace_root()` 都是 `/workspace`，
/// 按路径做键会让不同会话的 git 状态互相串。
const GIT_STATUS_CACHE_TTL: Duration = Duration::from_secs(2);
static GIT_STATUS_CACHE: Mutex<Option<HashMap<String, (Instant, Vec<GitChange>)>>> =
    Mutex::new(None);

pub fn git_status_response(
    env: &dyn crate::env::Environment,
    workspace_root: &Path,
) -> Result<GitStatusResponse> {
    // 缓存键只对 local 档位有效：沙箱档位下所有会话的 `workspace_root()` 都是
    // `/workspace`，按路径做键会把不同会话的 git 状态互相串（跨会话泄露文件名）。
    let cache_key = (env.kind() == crate::env::EnvKind::Local)
        .then(|| workspace_root.to_string_lossy().into_owned());
    if let Some(key) = cache_key.as_deref() {
        let cache = GIT_STATUS_CACHE.lock().unwrap();
        if let Some(entries) = cache.as_ref() {
            if let Some((at, changes)) = entries.get(key) {
                if at.elapsed() < GIT_STATUS_CACHE_TTL {
                    return Ok(GitStatusResponse {
                        changes: changes.clone(),
                        error: None,
                    });
                }
            }
        }
    }
    let result = git_status(env, workspace_root);
    if let (Some(key), Ok(changes)) = (cache_key, &result) {
        let mut cache = GIT_STATUS_CACHE.lock().unwrap();
        let entries = cache.get_or_insert_with(HashMap::new);
        entries.insert(key, (Instant::now(), changes.clone()));
    }
    match result {
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
    env: &dyn crate::env::Environment,
    workspace_root: &Path,
    relative_path: &str,
    status: Option<&str>,
) -> Result<GitDiff> {
    let root = panel_root(env, workspace_root)?;
    let relative = safe_relative(relative_path)?;
    if relative.as_os_str().is_empty() {
        return Err(anyhow!("file path is required"));
    }
    let path = relative.to_string_lossy().replace('\\', "/");
    let status = match status.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => resolve_git_status_for_path(env, &root, &path)?,
    };
    let mode = git_diff_mode(&status);
    let (before, after) = match mode {
        "untracked" => (String::new(), read_worktree_text(env, &root, &path)?),
        "unstaged" => (
            read_git_blob_text(env, &root, &format!(":{path}"))?.unwrap_or_default(),
            read_worktree_text(env, &root, &path)?,
        ),
        "staged" => (
            read_git_blob_text(env, &root, &format!("HEAD:{path}"))?.unwrap_or_default(),
            read_git_blob_text(env, &root, &format!(":{path}"))?.unwrap_or_default(),
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
    let staged = bytes.first().is_some_and(|c| *c != b'.' && *c != b'?');
    if staged {
        return "staged";
    }
    // Fallback: treat as unstaged worktree review.
    "unstaged"
}

fn resolve_git_status_for_path(
    env: &dyn crate::env::Environment,
    root: &Path,
    path: &str,
) -> Result<String> {
    let changes = git_status(env, root)?;
    changes
        .into_iter()
        .find(|c| c.path == path)
        .map(|c| c.status)
        .ok_or_else(|| anyhow!("path not in git status: {path}"))
}

fn read_git_blob_text(
    env: &dyn crate::env::Environment,
    root: &Path,
    spec: &str,
) -> Result<Option<String>> {
    match run_git(env, root, ["show", spec]) {
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

fn read_worktree_text(
    env: &dyn crate::env::Environment,
    root: &Path,
    relative: &str,
) -> Result<String> {
    let full = root.join(relative);
    if !env.exists(&full) {
        return Ok(String::new());
    }
    let meta = env
        .metadata(&full)
        .with_context(|| format!("stat {}", full.display()))?;
    if !meta.is_file {
        return Err(anyhow!("不是常规文件: {}", full.display()));
    }
    if meta.len as usize > GIT_FULL_DIFF_MAX_BYTES {
        return Err(anyhow!(
            "文件过大（>{} bytes），无法展示整文件 Diff",
            GIT_FULL_DIFF_MAX_BYTES
        ));
    }
    let bytes = env
        .read_file(&full)
        .with_context(|| format!("read {}", full.display()))?;
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

fn run_git<I, S>(env: &dyn crate::env::Environment, root: &Path, args: I) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut argv: Vec<String> = vec![
        "git".to_string(),
        "-C".to_string(),
        root.to_string_lossy().into_owned(),
    ];
    argv.extend(
        args.into_iter()
            .map(|a| a.as_ref().to_string_lossy().into_owned()),
    );
    let output = env
        .exec(&argv, crate::env::ExecOptions::new())
        .map_err(git_exec_error)?;
    parse_git_output(output)
}

/// 启动失败（git 未安装）与命令失败要区分：前端按 code 展示不同提示。
fn git_exec_error(error: anyhow::Error) -> anyhow::Error {
    let not_found = error.chain().find_map(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .filter(|io_err| io_err.kind() == io::ErrorKind::NotFound)
    });
    if let Some(io_err) = not_found {
        return git_spawn_error(io::Error::new(io::ErrorKind::NotFound, io_err.to_string()));
    }
    GitCommandError(GitErrorInfo {
        code: GitErrorCode::CommandFailed,
        message: format!("无法运行 Git：{error}"),
    })
    .into()
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

fn parse_git_output(output: crate::env::ExecOutput) -> Result<Vec<u8>> {
    if output.status == 0 {
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

    /// 宿主环境（面板在 local 档位下拿到的就是它）。
    fn host(root: &Path) -> std::sync::Arc<dyn crate::env::Environment> {
        crate::env::host_env_at(root)
    }

    #[test]
    fn rejects_paths_outside_workspace() {
        let root = tempfile::tempdir().unwrap();
        let env = host(root.path());
        let env = env.as_ref();
        assert!(list_directory(env, root.path(), Some("../outside")).is_err());
        assert!(git_diff(env, root.path(), "/tmp/outside", None).is_err());
        assert!(delete_path(env, root.path(), "../outside").is_err());
        assert!(delete_path(env, root.path(), "").is_err());
        assert!(resolve_file_for_media(env, root.path(), "../outside").is_err());
        assert!(resolve_file_for_media(env, root.path(), "").is_err());
    }

    #[test]
    fn resolve_file_for_media_returns_jpeg_mime() {
        let root = tempfile::tempdir().unwrap();
        let env = host(root.path());
        let file = root.path().join("shot.jpg");
        fs::write(&file, b"\xff\xd8\xff").unwrap();
        let media = resolve_file_for_media(env.as_ref(), root.path(), "shot.jpg").unwrap();
        assert_eq!(media.mime_type, "image/jpeg");
        assert_eq!(media.file_name, "shot.jpg");
        assert_eq!(media.path, file.canonicalize().unwrap());
    }

    /// 沙箱风格环境：路径是环境内路径（`/workspace/...`），IO 走内存假文件系统。
    ///
    /// `map` 由用例决定：容器后端有「guest `/workspace` → 宿主目录」的挂载表，
    /// 云端 envd 的挂载点只是模板（宿主上没有文件）。
    struct SandboxEnv {
        workspace: PathBuf,
        map: crate::env::PathMap,
        files: std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>,
        reads: std::sync::atomic::AtomicUsize,
    }

    impl SandboxEnv {
        fn new(mounts: Vec<crate::env::Mount>) -> Self {
            Self {
                workspace: PathBuf::from("/workspace"),
                map: crate::env::PathMap::new(mounts),
                files: std::sync::Mutex::new(std::collections::BTreeMap::new()),
                reads: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn put(&self, guest: &str, bytes: &[u8]) {
            self.files
                .lock()
                .expect("files")
                .insert(guest.to_string(), bytes.to_vec());
        }

        /// 环境读文件次数：用来证明「映射成功但缺文件」没有回落去读环境。
        fn reads(&self) -> usize {
            self.reads.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn lookup(&self, path: &Path) -> Option<Vec<u8>> {
            self.files
                .lock()
                .expect("files")
                .get(&path.to_string_lossy().into_owned())
                .cloned()
        }
    }

    impl crate::env::Environment for SandboxEnv {
        fn kind(&self) -> crate::env::EnvKind {
            crate::env::EnvKind::Headless
        }
        fn workspace_root(&self) -> &Path {
            &self.workspace
        }
        fn skills_root(&self) -> &Path {
            Path::new("/skills")
        }
        fn home_root(&self) -> &Path {
            Path::new("/home/pointer")
        }
        fn capabilities(&self) -> crate::env::Capabilities {
            crate::env::Capabilities::headless()
        }
        fn path_map(&self) -> &crate::env::PathMap {
            &self.map
        }
        fn exec(
            &self,
            _argv: &[String],
            _opts: crate::env::ExecOptions,
        ) -> Result<crate::env::ExecOutput> {
            Err(anyhow!("sandbox env: exec not supported"))
        }
        fn spawn(
            &self,
            _argv: &[String],
            _opts: crate::env::SpawnOptions,
        ) -> Result<Box<dyn crate::env::ChildHandle>> {
            Err(anyhow!("sandbox env: spawn not supported"))
        }
        fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
            self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.lookup(path)
                .ok_or_else(|| anyhow!("sandbox env: not found: {}", path.display()))
        }
        fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
            self.put(&path.to_string_lossy(), data);
            Ok(())
        }
        fn list_dir(&self, _path: &Path) -> Result<Vec<crate::env::DirEntry>> {
            Ok(Vec::new())
        }
        fn metadata(&self, path: &Path) -> Result<crate::env::FileMetadata> {
            let bytes = self
                .lookup(path)
                .ok_or_else(|| anyhow!("sandbox env: not found: {}", path.display()))?;
            Ok(crate::env::FileMetadata {
                is_file: true,
                is_dir: false,
                is_symlink: false,
                len: bytes.len() as u64,
            })
        }
        fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
            Ok(path.to_path_buf())
        }
        fn create_dir_all(&self, _path: &Path) -> Result<()> {
            Ok(())
        }
        fn read_to_string(&self, path: &Path) -> Result<String> {
            let bytes = self.read_file(path)?;
            Ok(String::from_utf8(bytes)?)
        }
        fn open_reader(&self, path: &Path) -> Result<Box<dyn std::io::Read + Send>> {
            let bytes = self.read_file(path)?;
            Ok(Box::new(std::io::Cursor::new(bytes)))
        }
        fn walk(
            &self,
            _root: &Path,
            _opts: crate::env::WalkOptions,
        ) -> Result<Vec<crate::env::WalkEntry>> {
            Ok(Vec::new())
        }
        fn remove_file(&self, _path: &Path) -> Result<()> {
            Ok(())
        }
        fn remove_dir_all(&self, _path: &Path) -> Result<()> {
            Ok(())
        }
        fn set_executable(&self, _path: &Path) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn media_body_reads_via_env_for_sandbox_mount() {
        // 容器后端即使有 bind mount，媒体字节也只经 Environment，不经宿主 fs。
        let host_ws = tempfile::tempdir().unwrap();
        let file = host_ws.path().join("pic.png");
        fs::write(&file, b"\x89PNG\r\n").unwrap();
        let env = SandboxEnv::new(vec![crate::env::Mount::new(
            host_ws.path(),
            "/workspace",
            false,
        )]);
        env.put("/workspace/pic.png", b"\x89PNG\r\n");

        let media = resolve_file_for_media(&env, Path::new("/workspace"), "pic.png").unwrap();
        assert_eq!(
            media.path,
            PathBuf::from("/workspace/pic.png"),
            "沙箱里拿到的是 guest 路径"
        );

        let body =
            media_body_for_streaming(&env, &media.path, WORKSPACE_MEDIA_INLINE_MAX_BYTES).unwrap();
        assert_eq!(body, WorkspaceMediaBody::Inline(b"\x89PNG\r\n".to_vec()));
        assert_eq!(body.source_label(), "env-inline");
        assert!(env.reads() >= 1, "沙箱必须经环境读文件");
    }

    #[test]
    fn media_body_reads_env_bytes_when_host_has_no_file() {
        // 云端 envd：挂载点只是模板，宿主目录里没有这个文件 → 整段读进内存。
        let host_ws = tempfile::tempdir().unwrap();
        let env = SandboxEnv::new(vec![crate::env::Mount::new(
            host_ws.path(),
            "/workspace",
            false,
        )]);
        env.put("/workspace/out.mp4", b"video-bytes");

        let body = media_body_for_streaming(
            &env,
            Path::new("/workspace/out.mp4"),
            WORKSPACE_MEDIA_INLINE_MAX_BYTES,
        )
        .unwrap();
        assert_eq!(body, WorkspaceMediaBody::Inline(b"video-bytes".to_vec()));
        assert_eq!(body.source_label(), "env-inline");
    }

    #[test]
    fn media_body_rejects_oversize_env_file_with_clear_error() {
        let env = SandboxEnv::new(Vec::new());
        env.put("/workspace/huge.mp4", &vec![7u8; 2048]);

        let err = media_body_for_streaming(&env, Path::new("/workspace/huge.mp4"), 1024)
            .expect_err("超过上限必须报错，不能静默截断");
        let msg = err.to_string();
        assert!(msg.contains("超过在线预览上限"), "{msg}");
        assert!(msg.contains("请下载"), "{msg}");
    }

    #[test]
    fn media_body_errors_when_env_file_missing() {
        // 沙箱只认环境内文件；不存在就报错，不猜宿主挂载。
        let host_ws = tempfile::tempdir().unwrap();
        let env = SandboxEnv::new(vec![crate::env::Mount::new(
            host_ws.path(),
            "/workspace",
            false,
        )]);

        let err = media_body_for_streaming(
            &env,
            Path::new("/workspace/missing.png"),
            WORKSPACE_MEDIA_INLINE_MAX_BYTES,
        )
        .expect_err("文件不存在应报错");
        assert!(
            err.to_string().contains("workspace file does not exist")
                || err.to_string().contains("not found"),
            "{err}"
        );
        assert_eq!(env.reads(), 0, "metadata 失败时不应再 read_file");
    }

    #[test]
    fn media_body_keeps_host_path_for_local_env() {
        // local 档位：路径映射恒等，行为与改动前完全一致（原样交给流式打开）。
        let root = tempfile::tempdir().unwrap();
        let env = host(root.path());
        let file = root.path().join("shot.png");
        fs::write(&file, b"\x89PNG").unwrap();

        let media = resolve_file_for_media(env.as_ref(), root.path(), "shot.png").unwrap();
        let body =
            media_body_for_streaming(env.as_ref(), &media.path, WORKSPACE_MEDIA_INLINE_MAX_BYTES)
                .unwrap();
        assert_eq!(body, WorkspaceMediaBody::HostPath(media.path.clone()));
        assert_eq!(body.source_label(), "host-stream");
    }

    #[test]
    fn deletes_files_and_directories_inside_workspace() {
        let root = tempfile::tempdir().unwrap();
        let env = host(root.path());
        let env = env.as_ref();
        fs::create_dir(root.path().join("folder")).unwrap();
        fs::write(root.path().join("folder/nested.txt"), b"nested").unwrap();
        fs::write(root.path().join("file.txt"), b"hello").unwrap();

        delete_path(env, root.path(), "file.txt").unwrap();
        assert!(!root.path().join("file.txt").exists());

        delete_path(env, root.path(), "folder").unwrap();
        assert!(!root.path().join("folder").exists());
        assert!(delete_path(env, root.path(), "missing.txt").is_err());
    }

    #[test]
    fn search_entries_matches_nested_names_and_skips_node_modules() {
        let root = tempfile::tempdir().unwrap();
        let env = host(root.path());
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/App.vue"), b"app").unwrap();
        fs::create_dir(root.path().join("node_modules")).unwrap();
        fs::write(root.path().join("node_modules/hidden.ts"), b"x").unwrap();

        let hits = search_entries(env.as_ref(), root.path(), "app", 50).unwrap();
        assert!(hits.iter().any(|e| e.path == "src/App.vue"));
        assert!(!hits.iter().any(|e| e.path.contains("node_modules")));
    }

    #[test]
    fn lists_only_one_level_and_does_not_follow_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let env = host(root.path());
        fs::create_dir(root.path().join("folder")).unwrap();
        fs::write(root.path().join("file.txt"), b"hello").unwrap();
        fs::write(root.path().join("folder/nested.txt"), b"nested").unwrap();
        let entries = list_directory(env.as_ref(), root.path(), None).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, WorkspaceEntryKind::Directory);
        assert_eq!(entries[1].size_bytes, Some(5));
    }

    #[test]
    fn reads_text_preview_and_rejects_binary_or_escaped_paths() {
        let root = tempfile::tempdir().unwrap();
        let env = host(root.path());
        let env = env.as_ref();
        fs::write(root.path().join("file.txt"), "hello\n世界\n").unwrap();
        fs::write(root.path().join("binary.bin"), b"abc\0def").unwrap();

        let text = read_file(env, root.path(), "file.txt").unwrap();
        assert_eq!(text.content.as_deref(), Some("hello\n世界\n"));
        assert!(!text.binary);
        assert!(!text.truncated);

        let binary = read_file(env, root.path(), "binary.bin").unwrap();
        assert!(binary.binary);
        assert!(binary.content.is_none());
        assert!(read_file(env, root.path(), "../outside.txt").is_err());
    }

    #[test]
    fn truncates_large_text_preview_on_a_utf8_boundary() {
        let root = tempfile::tempdir().unwrap();
        let env = host(root.path());
        let content = format!("{}界", "a".repeat(WORKSPACE_FILE_PREVIEW_MAX_BYTES - 1));
        fs::write(root.path().join("large.txt"), content).unwrap();

        let preview = read_file(env.as_ref(), root.path(), "large.txt").unwrap();
        assert!(preview.truncated);
        assert!(!preview.binary);
        assert_eq!(
            preview.content.as_ref().unwrap().len(),
            WORKSPACE_FILE_PREVIEW_MAX_BYTES - 1
        );
    }

    #[test]
    fn classifies_git_command_failures_without_matching_in_the_frontend() {
        let output = crate::env::host_env()
            .exec(
                &[
                    "git".to_string(),
                    "-C".to_string(),
                    "/definitely/not/a/workspace".to_string(),
                    "status".to_string(),
                ],
                crate::env::ExecOptions::new(),
            )
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
        let env = host(root.path());
        let env = env.as_ref();
        run_git(env, root.path(), ["init"]).unwrap();
        run_git(
            env,
            root.path(),
            ["config", "user.email", "test@example.com"],
        )
        .unwrap();
        run_git(env, root.path(), ["config", "user.name", "Test"]).unwrap();
        fs::write(root.path().join("a.txt"), "one\n").unwrap();
        run_git(env, root.path(), ["add", "a.txt"]).unwrap();
        run_git(env, root.path(), ["commit", "-m", "initial"]).unwrap();
        fs::write(root.path().join("a.txt"), "two\n").unwrap();
        run_git(env, root.path(), ["add", "a.txt"]).unwrap();
        fs::write(root.path().join("a.txt"), "three\n").unwrap();

        let status = git_status(env, root.path()).unwrap();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].status, "MM");
        assert!(status[0].staged);

        // MM prefers unstaged: index ("two") → worktree ("three")
        let unstaged = git_diff(env, root.path(), "a.txt", Some("MM")).unwrap();
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
        let staged = git_diff(env, root.path(), "a.txt", Some("M.")).unwrap();
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

    #[test]
    fn panel_does_not_read_host_workspace_when_environment_is_light() {
        // headless 档位下没有绑定/绑定为沙箱时，面板绝不能去读宿主目录。
        let env = crate::env::test_light_env();
        let host_dir = std::env::temp_dir();
        let err = list_directory(env.as_ref(), &host_dir, None)
            .expect_err("headless 环境不应读到宿主目录");
        let chain = format!("{err:#}");
        assert!(chain.contains("未绑定"), "错误链应说明环境未绑定：{chain}");
    }
}
