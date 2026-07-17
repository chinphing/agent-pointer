use anyhow::{anyhow, Result};
use globset::{Glob, GlobSetBuilder};
use std::cell::RefCell;
use std::fs;
use std::path::{Component, Path, PathBuf};

// Predefined file type globs (align with ripgrep; small curated set).
const FILE_TYPE_GLOBS: &[(&str, &[&str])] = &[
    ("rust", &["*.rs"]),
    ("toml", &["*.toml"]),
    ("py", &["*.py", "*.pyi"]),
    ("js", &["*.js", "*.jsx", "*.cjs", "*.mjs"]),
    ("ts", &["*.ts", "*.tsx", "*.mts", "*.cts"]),
    ("vue", &["*.vue"]),
    ("tsx", &["*.tsx"]),
    ("svelte", &["*.svelte"]),
    ("md", &["*.md", "*.mdx"]),
    ("json", &["*.json", "*.jsonc"]),
    ("yaml", &["*.yml", "*.yaml"]),
    ("html", &["*.html", "*.htm"]),
    ("css", &["*.css", "*.scss", "*.less"]),
    ("xml", &["*.xml", "*.xsl", "*.xslt"]),
    ("go", &["*.go"]),
    ("java", &["*.java"]),
    ("kt", &["*.kt", "*.kts"]),
    ("c", &["*.c", "*.h"]),
    ("cpp", &["*.cpp", "*.cc", "*.cxx", "*.hpp", "*.hh", "*.hxx"]),
    ("rb", &["*.rb", "*.rake", "*.gemspec"]),
    ("php", &["*.php", "*.phtml"]),
    ("swift", &["*.swift"]),
    ("scala", &["*.scala", "*.sc"]),
    ("sql", &["*.sql"]),
    ("sh", &["*.sh", "*.bash", "*.zsh"]),
];

/// Map user-facing type names (aliases, extensions, mixed case) to canonical keys in [`FILE_TYPE_GLOBS`].
fn normalize_file_type_name(name: &str) -> String {
    match name.trim().to_ascii_lowercase().as_str() {
        "rs" => "rust".to_string(),
        "python" => "py".to_string(),
        "javascript" => "js".to_string(),
        "typescript" => "ts".to_string(),
        "markdown" => "md".to_string(),
        "yml" => "yaml".to_string(),
        "kotlin" => "kt".to_string(),
        "c++" | "cxx" | "hpp" => "cpp".to_string(),
        "ruby" => "rb".to_string(),
        "shell" | "bash" | "zsh" => "sh".to_string(),
        other => other.to_string(),
    }
}

pub(crate) fn expand_file_types(types: &[String]) -> Result<Vec<String>> {
    let mut globs = Vec::new();
    for t in types {
        let canonical = normalize_file_type_name(t);
        let found = FILE_TYPE_GLOBS
            .iter()
            .find(|(name, _)| *name == canonical.as_str());
        match found {
            Some((_, g)) => globs.extend(g.iter().map(|s| s.to_string())),
            None => return Err(anyhow!("未知文件类型: {t}")),
        }
    }
    Ok(deduplicate_globs(globs))
}

pub(crate) fn deduplicate_globs(mut globs: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    globs.retain(|g| seen.insert(g.clone()));
    globs
}

pub(crate) fn build_glob_set(globs: Option<&[String]>) -> Result<Option<globset::GlobSet>> {
    if let Some(globs) = globs {
        if globs.is_empty() {
            Ok(None)
        } else {
            let mut builder = GlobSetBuilder::new();
            for g in globs {
                let glob = Glob::new(g).map_err(|e| anyhow!("无效 glob 模式 '{g}': {e}"))?;
                builder.add(glob);
            }
            let set = builder.build().map_err(|e| anyhow!("构建 glob set 失败: {e}"))?;
            Ok(Some(set))
        }
    } else {
        Ok(None)
    }
}

/// Expand `~` / `~/…` to the session user's home directory.
pub(crate) fn expand_user_path_for_file(user_path: &str) -> Result<String> {
    let s = normalize_user_fspath(user_path);
    if s.is_empty() {
        return Ok(String::new());
    }
    if s == "~" || s.starts_with("~/") || s.starts_with("~\\") {
        return crate::media::access::expand_root(s)
            .map(|p| p.to_string_lossy().into_owned());
    }
    Ok(s.to_string())
}

/// Trim and drop redundant trailing `/` or `\` so `.../mod.rs/` resolves like `.../mod.rs`.
/// On Windows, leaves `C:\` unchanged when that is the whole path after trimming separators.
fn normalize_user_fspath(user_path: &str) -> &str {
    let s = user_path.trim();
    if s.is_empty() {
        return s;
    }
    #[cfg(unix)]
    {
        let t = s.trim_end_matches('/');
        if t.is_empty() {
            s
        } else {
            t
        }
    }
    #[cfg(windows)]
    {
        let t = s.trim_end_matches(|c| c == '/' || c == '\\');
        if t.is_empty() {
            return s;
        }
        let b = t.as_bytes();
        if b.len() == 2 && b[1] == b':' {
            return s;
        }
        t
    }
}

/// Binary-ish extensions to skip in grep
pub(crate) const SKIP_EXT: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "ico", "pdf", "zip", "gz", "7z", "rar", "exe", "dll",
    "so", "dylib", "wasm", "mp3", "mp4", "avi", "mkv", "ttf", "woff", "woff2", "eot",
];
thread_local! {
    static CONVERSATION_WORKSPACE_ROOT: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Thread-local workspace override for an in-flight chat run (set for the whole `run_chat`).
pub struct ConversationWorkspaceGuard {
    previous: Option<String>,
}

impl ConversationWorkspaceGuard {
    pub fn enter(workspace: String) -> Self {
        let previous = CONVERSATION_WORKSPACE_ROOT.with(|c| {
            let mut g = c.borrow_mut();
            let next = if workspace.trim().is_empty() {
                None
            } else {
                Some(workspace)
            };
            std::mem::replace(&mut *g, next)
        });
        Self { previous }
    }
}

impl Drop for ConversationWorkspaceGuard {
    fn drop(&mut self) {
        CONVERSATION_WORKSPACE_ROOT.with(|c| {
            *c.borrow_mut() = self.previous.take();
        });
    }
}

/// Lightweight save/restore guard for sub-agent execution.
///
/// Wraps a single agent run (lead or sub-agent): saves the current thread-local
/// workspace root, sets it to `agent_workspace_root`, and restores the original
/// on drop.  This prevents sub-agents from contaminating the parent's workspace
/// in `CONVERSATION_WORKSPACE_ROOT`.
pub struct AgentWorkspaceGuard {
    previous: Option<String>,
}

impl AgentWorkspaceGuard {
    pub fn enter(agent_workspace_root: &str) -> Self {
        let trimmed = agent_workspace_root.trim();
        let next = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
        let previous = CONVERSATION_WORKSPACE_ROOT.with(|c| {
            std::mem::replace(&mut *c.borrow_mut(), next)
        });
        Self { previous }
    }
}

impl Drop for AgentWorkspaceGuard {
    fn drop(&mut self) {
        CONVERSATION_WORKSPACE_ROOT.with(|c| {
            *c.borrow_mut() = self.previous.take();
        });
    }
}

pub fn workspace_root_from_override_or_settings() -> String {
    CONVERSATION_WORKSPACE_ROOT
        .with(|c| c.borrow().clone())
        .unwrap_or_else(|| {
            crate::platform_config::effective_settings_global()
                .workspace_root
                .clone()
        })
}

/// Update the in-flight conversation workspace (e.g. before delegating to **coder**).
pub fn set_runtime_workspace_root(root: String) {
    let trimmed = root.trim();
    CONVERSATION_WORKSPACE_ROOT.with(|c| {
        *c.borrow_mut() = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
    });
}

pub fn resolve_tool_workspace_root() -> Result<PathBuf> {
    let raw = workspace_root_from_override_or_settings();
    let raw = raw.trim();
    if !raw.is_empty() {
        let p = PathBuf::from(raw);
        if !p.is_dir() {
            return Err(anyhow!("工作区目录无效或不存在: {}", raw));
        }
        return p
            .canonicalize()
            .map_err(|e| anyhow!("无法解析工作区路径: {e}"));
    }
    std::env::current_dir().map_err(|e| anyhow!("无法获取当前目录: {e}"))
}

fn push_writable_root(roots: &mut Vec<PathBuf>, path: PathBuf, pinned: Option<&Path>) {
    if path.as_os_str().is_empty() {
        return;
    }
    let canon = path.canonicalize().unwrap_or(path);
    if roots.iter().any(|r| canon.starts_with(r)) {
        return;
    }
    // Never demote the conversation workspace when a broader root (home / temp_dir) is merged.
    roots.retain(|r| pinned.is_some_and(|p| r == p) || !r.starts_with(&canon));
    if roots.iter().any(|r| r == &canon) {
        return;
    }
    roots.push(canon);
}

/// Allowed write roots for `file_write` / `file_edit`: workspace, home, temp, and common user data dirs.
///
/// The conversation workspace is always kept as the first entry and is never removed when
/// broader roots (home, `temp_dir`, …) are merged — nested temp workspaces would otherwise
/// disappear from the list and break any consumer that assumes `roots[0]` is the workspace.
pub fn writable_path_roots(workspace_root: &Path) -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    let ws = workspace_root
        .canonicalize()
        .map_err(|e| anyhow!("工作区根无效: {e}"))?;
    push_writable_root(&mut roots, ws.clone(), None);
    let pinned = roots.first().cloned();
    let pinned_ref = pinned.as_deref();

    if let Some(home) = dirs::home_dir() {
        push_writable_root(&mut roots, home, pinned_ref);
    }
    push_writable_root(&mut roots, std::env::temp_dir(), pinned_ref);

    for dir in [
        dirs::data_dir(),
        dirs::config_dir(),
        dirs::cache_dir(),
        dirs::desktop_dir(),
        dirs::document_dir(),
        dirs::download_dir(),
    ]
    .into_iter()
    .flatten()
    {
        push_writable_root(&mut roots, dir, pinned_ref);
    }

    if let Ok(app) = crate::storage::app_data_dir() {
        push_writable_root(&mut roots, app, pinned_ref);
    }

    if roots.is_empty() {
        return Err(anyhow!("无法解析允许的写入目录"));
    }
    Ok(roots)
}

fn path_under_any_root(candidate: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| candidate.starts_with(root))
}

fn resolve_absolute_under_roots(roots: &[PathBuf], abs: &Path) -> Result<PathBuf> {
    let abs_owned = abs.to_path_buf();
    let mut probe = abs_owned.clone();
    loop {
        if probe.as_os_str().is_empty() {
            return Err(anyhow!("绝对路径不在允许的写入目录内"));
        }
        if probe.exists() {
            let base = probe
                .canonicalize()
                .map_err(|e| anyhow!("绝对路径无效: {e}"))?;
            if !path_under_any_root(&base, roots) {
                return Err(anyhow!("绝对路径不在允许的写入目录内"));
            }
            let suffix = abs_owned
                .strip_prefix(&probe)
                .map_err(|_| anyhow!("绝对路径前缀解析失败"))?;
            for c in suffix.components() {
                match c {
                    Component::Normal(_) | Component::CurDir => {}
                    Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                        return Err(anyhow!("绝对路径的尾部含非法路径组件"));
                    }
                }
            }
            let candidate = if suffix.as_os_str().is_empty() {
                base
            } else {
                base.join(suffix)
            };
            if !path_under_any_root(&candidate, roots) {
                return Err(anyhow!("路径不在允许的写入目录内"));
            }
            return Ok(candidate);
        }
        if !probe.pop() {
            return Err(anyhow!(
                "绝对路径不在允许的写入目录内（与已知目录无共同已存在路径）"
            ));
        }
    }
}

fn resolve_relative_under_workspace(root: &Path, user_path: &str) -> Result<PathBuf> {
    let user_path = normalize_user_fspath(user_path);
    if user_path.is_empty() {
        return Err(anyhow!("路径不能为空"));
    }
    if user_path.contains('\0') {
        return Err(anyhow!("路径含非法字符"));
    }
    let path = Path::new(user_path);
    if path.is_absolute() {
        return Err(anyhow!("相对路径含非法根组件"));
    }
    let mut acc = root.to_path_buf();
    for c in path.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => {
                return Err(anyhow!("相对路径含非法根组件"));
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !acc.pop() {
                    return Err(anyhow!("路径越出工作区"));
                }
                if !acc.starts_with(root) {
                    return Err(anyhow!("路径越出工作区"));
                }
            }
            Component::Normal(s) => acc.push(s),
        }
    }
    if !acc.starts_with(root) {
        return Err(anyhow!("路径不在工作区内"));
    }
    Ok(acc)
}

/// Resolve `user_path` for `file_write` / `file_edit`.
///
/// Relative paths stay under the **conversation workspace** (`workspace_root` canonicalized).
/// Do not derive the relative base from other writable roots; [`writable_path_roots`] keeps
/// the workspace pinned at index 0, but callers should still pass `workspace_root` explicitly.
///
/// Absolute paths (including expanded `~`) may target workspace, user home, system temp,
/// standard user data dirs, or Pointer app data.
pub fn resolve_writable_path(workspace_root: &Path, user_path: &str) -> Result<PathBuf> {
    let roots = writable_path_roots(workspace_root)?;
    let workspace = workspace_root
        .canonicalize()
        .map_err(|e| anyhow!("工作区根无效: {e}"))?;
    let expanded = expand_user_path_for_file(user_path)?;
    if expanded.is_empty() {
        return Err(anyhow!("路径不能为空"));
    }
    if expanded.contains('\0') {
        return Err(anyhow!("路径含非法字符"));
    }
    let path = Path::new(expanded.as_str());

    let out = if path.is_absolute() {
        resolve_absolute_under_roots(&roots, path)?
    } else {
        resolve_relative_under_workspace(&workspace, &expanded)?
    };

    if !path_under_any_root(&out, &roots) {
        return Err(anyhow!("路径不在允许的写入目录内"));
    }
    Ok(out)
}

/// Resolve an **absolute** path for `file:write` / `file:edit`: must stay under canonical `root`.
/// The target file (or missing parent dirs) may not exist yet; resolution walks up to an
fn resolve_absolute_under_workspace(root: &Path, abs: &Path) -> Result<PathBuf> {
    resolve_absolute_under_roots(&[root
        .canonicalize()
        .map_err(|e| anyhow!("工作区根无效: {e}"))?], abs)
}

/// Resolve `user_path` (relative to root, or absolute but must stay under canonical `root`).
/// Rejects traversal outside root.
pub fn resolve_within_workspace_root(root: &Path, user_path: &str) -> Result<PathBuf> {
    let root = root
        .canonicalize()
        .map_err(|e| anyhow!("工作区根无效: {e}"))?;
    let user_path = normalize_user_fspath(user_path);
    if user_path.is_empty() {
        return Err(anyhow!("路径不能为空"));
    }
    if user_path.contains('\0') {
        return Err(anyhow!("路径含非法字符"));
    }
    let path = Path::new(user_path);

    let out = if path.is_absolute() {
        resolve_absolute_under_workspace(&root, path)?
    } else {
        let mut acc = root.clone();
        for c in path.components() {
            match c {
                Component::Prefix(_) | Component::RootDir => {
                    return Err(anyhow!("相对路径含非法根组件"));
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    if !acc.pop() {
                        return Err(anyhow!("路径越出工作区"));
                    }
                    if !acc.starts_with(&root) {
                        return Err(anyhow!("路径越出工作区"));
                    }
                }
                Component::Normal(s) => acc.push(s),
            }
        }
        acc
    };

    if !out.starts_with(&root) {
        return Err(anyhow!("路径不在工作区内"));
    }
    Ok(out)
}

/// Absolute path string for tool JSON responses. Prefer [`Path::canonicalize`] when it succeeds.
pub(crate) fn path_display_abs(path: &Path) -> String {
    path.canonicalize()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

const MAX_PATH_HINTS: usize = 8;

fn levenshtein_ascii(a: &str, b: &str) -> u32 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len() as u32;
    }
    if b.is_empty() {
        return a.len() as u32;
    }
    let mut prev: Vec<u32> = (0..=b.len()).map(|i| i as u32).collect();
    let mut curr = vec![0u32; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = (i + 1) as u32;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1)
                .min(curr[j] + 1)
                .min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

fn path_name_similarity(want: &str, candidate: &str) -> u32 {
    let a = want.trim().to_ascii_lowercase();
    let b = candidate.trim().to_ascii_lowercase();
    if a.is_empty() {
        return u32::MAX;
    }
    if a == b {
        return 0;
    }
    if b.contains(&a) || a.contains(&b) {
        return 1;
    }
    let dist = levenshtein_ascii(&a, &b);
    if dist <= 3 {
        2 + dist
    } else {
        u32::MAX
    }
}

/// Parent directory to list when `user_path` does not exist, plus the missing final name.
fn listing_base_for_missing_path(workspace_root: &Path, user_path: &str) -> Option<(PathBuf, String)> {
    let user_path = normalize_user_fspath(user_path);
    let path = Path::new(user_path);
    let want = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(user_path)
        .to_string();

    if path.is_absolute() {
        let mut probe = path.to_path_buf();
        loop {
            if probe.exists() && probe.is_dir() {
                let base = probe.canonicalize().unwrap_or(probe);
                return Some((base, want));
            }
            if !probe.pop() {
                break;
            }
        }
        let ws = workspace_root.canonicalize().ok()?;
        let probe_str = user_path.replace('\\', "/");
        let ws_str = ws.to_string_lossy().replace('\\', "/");
        if probe_str.starts_with(&ws_str) {
            return Some((ws, want));
        }
        None
    } else {
        let root = workspace_root.canonicalize().ok()?;
        let base = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .and_then(|parent| resolve_within_workspace_root(&root, parent.to_str()?).ok())
            .filter(|p| p.exists() && p.is_dir())
            .map(|p| p.canonicalize().unwrap_or(p))
            .unwrap_or(root);
        Some((base, want))
    }
}

fn collect_path_hints(workspace_root: &Path, user_path: &str) -> Vec<String> {
    let Some((list_base, want)) = listing_base_for_missing_path(workspace_root, user_path) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&list_base) else {
        return Vec::new();
    };

    let mut scored: Vec<(u32, String)> = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let score = if p.is_dir() {
            path_name_similarity(&want, &name)
        } else if p.is_file() {
            path_name_similarity(&want, &name).saturating_add(10)
        } else {
            continue;
        };
        if score < u32::MAX {
            scored.push((score, path_display_abs(&p)));
        }
    }
    scored.sort_by_key(|(s, p)| (*s, p.clone()));
    scored.dedup_by(|a, b| a.1 == b.1);
    if scored.is_empty() {
        if let Ok(entries) = fs::read_dir(&list_base) {
            for entry in entries.flatten() {
                if scored.len() >= MAX_PATH_HINTS {
                    break;
                }
                let p = entry.path();
                if p.is_dir() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if !name.starts_with('.') {
                        scored.push((u32::MAX, path_display_abs(&p)));
                    }
                }
            }
        }
    } else {
        scored.truncate(MAX_PATH_HINTS);
    }
    scored.into_iter().map(|(_, p)| p).collect()
}

pub(crate) fn path_error_with_hints(
    workspace_root: &Path,
    user_path: &str,
    message: impl std::fmt::Display,
) -> anyhow::Error {
    let hints = collect_path_hints(workspace_root, user_path);
    if hints.is_empty() {
        anyhow!("{message}")
    } else {
        anyhow!("{message}\n可能的路径: {}", hints.join(", "))
    }
}

/// Resolve a path for read-only tools that require an existing file or directory.
pub(crate) fn resolve_existing_read_path(workspace_root: &Path, user_path: &str, purpose: &str) -> Result<PathBuf> {
    let p = resolve_accessible_path(workspace_root, user_path).map_err(|e| {
        path_error_with_hints(
            workspace_root,
            user_path,
            format!("{purpose} 路径无效: {e}"),
        )
    })?;
    if p.exists() {
        Ok(p)
    } else {
        Err(path_error_with_hints(
            workspace_root,
            user_path,
            format!("{purpose} 路径不存在: {}", user_path.trim()),
        ))
    }
}

/// Best-effort absolute display for a user-supplied path in read errors (batch budget skips, etc.).
pub(crate) fn path_display_for_read_request(workspace_root: &Path, path_str: &str) -> String {
    resolve_accessible_path(workspace_root, path_str)
        .map(|p| path_display_abs(&p))
        .unwrap_or_else(|_| path_str.to_string())
}

/// Resolve paths for **read-only** `file` methods. Relative paths must stay under `workspace_root`.
/// **Absolute** paths are canonicalized as-is so other projects can be read when the user provides them.
pub fn resolve_accessible_path(workspace_root: &Path, user_path: &str) -> Result<PathBuf> {
    let workspace_root = workspace_root
        .canonicalize()
        .map_err(|e| anyhow!("工作区根无效: {e}"))?;
    let user_path = expand_user_path_for_file(user_path)?;
    if user_path.is_empty() {
        return Err(anyhow!("路径不能为空"));
    }
    if user_path.contains('\0') {
        return Err(anyhow!("路径含非法字符"));
    }
    let path = Path::new(user_path.as_str());
    if path.is_absolute() {
        return path
            .canonicalize()
            .map_err(|e| anyhow!("路径无效或不存在: {e}"));
    }
    resolve_within_workspace_root(&workspace_root, user_path.as_str())
}
