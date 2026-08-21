//! `AGENTS.md` 工程指令（设计稿 §4.4 / §4.2）。
//!
//! 对齐 Codex：全局一份 + 从 git 根沿工作区路径一路拼接，注入
//! [`ExtensionRegistry::MessageLoopPromptsAfter`]。
//!
//! 规则：
//! - 全局：`~/.pointer/AGENTS.md`（`dirs::home_dir()`，跨平台）；
//! - 项目：会话工作区当作 cwd（不使用进程 cwd）。从该目录向上找 `.git`
//!   （目录或 worktree 文件），再从 git 根沿路径走到工作区，每层最多一份；
//! - 找不到 git 根时只读工作区根上的那一份；
//! - 合并顺序：全局 → git 根 → … → 工作区（近处在后）；同一路径只注入一次；
//! - 不扫描旁支子目录，也不按当前打开的文件动态附加；
//! - 不读文件系统根（`/` / `C:\`）上的同名文件；
//! - 发现 ≠ 执行：仅读取文本注入 Prompt，不改变任何运行时行为。

use crate::extensions::{
    new_extension_message_id, now_ms, MessageLoopPromptsAfterContext, MessageLoopPromptsAfterHook,
};
use crate::models::{ChatMessage, Role};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// AGENTS.md 文件名。
pub const AGENTS_MD: &str = "AGENTS.md";
/// Extension hook 的 override_key（固定单例，重复注册替换）。
const HOOK_KEY: &str = "workspace_agents_md";
/// 注入排序（在 task board 等动态注入之前、内置钩子之后）。
const SORT_KEY: &str = "_60_agents_md";

const GLOBAL_LABEL: &str = "~/.pointer/AGENTS.md";
const WORKSPACE_LABEL: &str = "AGENTS.md";
/// Stop walking parents / git-root→cwd components (typical repos are far shallower).
const MAX_ANCESTORS: usize = 64;

fn is_filesystem_root(path: &Path) -> bool {
    let mut comps = path.components();
    match comps.next() {
        Some(std::path::Component::RootDir) => comps.next().is_none(),
        Some(std::path::Component::Prefix(_)) => {
            matches!(comps.next(), Some(std::path::Component::RootDir)) && comps.next().is_none()
        }
        _ => false,
    }
}

fn pointer_home_dir_no_create() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".pointer"))
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

fn posix_rel(root: &Path, file: &Path) -> String {
    match file.strip_prefix(root) {
        Ok(rel) => rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => AGENTS_MD.to_string(),
    }
}

fn path_has_git_marker(dir: &Path) -> bool {
    match std::fs::symlink_metadata(dir.join(".git")) {
        Ok(_) => true,
        Err(e) if e.kind() == ErrorKind::NotFound => false,
        Err(e) => {
            log::warn!(
                "agents_md: .git metadata failed dir={} err={e}",
                dir.display()
            );
            false
        }
    }
}

/// Walk up from `start` until `.git` exists. Filesystem root is never a git root.
fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut cursor = start.to_path_buf();
    for depth in 0..=MAX_ANCESTORS {
        if is_filesystem_root(&cursor) {
            log::info!(
                "agents_md: stop at filesystem root, no git marker start={}",
                start.display()
            );
            return None;
        }
        if path_has_git_marker(&cursor) {
            log::info!(
                "agents_md: git root={} depth={} start={}",
                cursor.display(),
                depth,
                start.display()
            );
            return Some(cursor);
        }
        match cursor.parent() {
            Some(p) => cursor = p.to_path_buf(),
            None => return None,
        }
    }
    log::warn!(
        "agents_md: ancestor walk exceeded max={} start={}",
        MAX_ANCESTORS,
        start.display()
    );
    None
}

/// Directories from git root down to cwd (inclusive). No git → cwd only. Never walks siblings.
fn project_search_dirs(cwd: &Path) -> (Option<PathBuf>, Vec<PathBuf>) {
    if is_filesystem_root(cwd) {
        log::warn!(
            "agents_md: skip filesystem root workspace={}",
            cwd.display()
        );
        return (None, Vec::new());
    }
    let cwd = match cwd.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            log::warn!(
                "agents_md: canonicalize failed path={} err={e}",
                cwd.display()
            );
            cwd.to_path_buf()
        }
    };
    if is_filesystem_root(&cwd) {
        log::warn!(
            "agents_md: skip filesystem root workspace={}",
            cwd.display()
        );
        return (None, Vec::new());
    }
    let Some(git_root) = find_git_root(&cwd) else {
        log::info!("agents_md: no git root, cwd only cwd={}", cwd.display());
        return (None, vec![cwd]);
    };
    if !cwd.starts_with(&git_root) {
        log::warn!(
            "agents_md: cwd not under git root, cwd only cwd={} git_root={}",
            cwd.display(),
            git_root.display()
        );
        return (None, vec![cwd]);
    }
    let mut dirs = Vec::new();
    let mut cursor = git_root.clone();
    loop {
        dirs.push(cursor.clone());
        if cursor == cwd {
            break;
        }
        let Ok(rel) = cwd.strip_prefix(&cursor) else {
            log::warn!(
                "agents_md: strip_prefix failed cursor={} cwd={}",
                cursor.display(),
                cwd.display()
            );
            break;
        };
        let Some(next) = rel.components().next() else {
            break;
        };
        cursor = cursor.join(next);
        if dirs.len() > MAX_ANCESTORS {
            log::warn!(
                "agents_md: git-root-to-cwd walk exceeded max={} cwd={}",
                MAX_ANCESTORS,
                cwd.display()
            );
            break;
        }
    }
    (Some(git_root), dirs)
}

/// 目录下的 `AGENTS.md`（文件或指向文件的符号链接）。目录 / 非常规文件记 warn。
fn agents_md_in_dir(dir: &Path) -> Option<PathBuf> {
    if is_filesystem_root(dir) {
        log::warn!("agents_md: skip filesystem root root={}", dir.display());
        return None;
    }
    let path = dir.join(AGENTS_MD);
    match std::fs::symlink_metadata(&path) {
        Ok(md) => {
            let ft = md.file_type();
            if ft.is_file() || ft.is_symlink() {
                Some(path)
            } else if ft.is_dir() {
                log::warn!(
                    "agents_md: skip, path is a directory path={}",
                    path.display()
                );
                None
            } else {
                log::warn!(
                    "agents_md: skip, not a regular file path={}",
                    path.display()
                );
                None
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(e) => {
            log::warn!(
                "agents_md: metadata failed path={} err={e}",
                path.display()
            );
            None
        }
    }
}

fn already_have(out: &[(String, PathBuf)], path: &Path) -> bool {
    out.iter().any(|(_, p)| p == path || same_dir(p, path))
}

/// 全局 + git 根→工作区路径。`pointer_home` 为 `None` 时跳过全局层。
pub fn discover_agents_md_chain(
    workspace_root: Option<&Path>,
    pointer_home: Option<&Path>,
) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    if let Some(home) = pointer_home {
        if let Some(path) = agents_md_in_dir(home) {
            out.push((GLOBAL_LABEL.to_string(), path));
        }
    }
    let Some(cwd) = workspace_root else {
        return out;
    };
    let (git_root, dirs) = project_search_dirs(cwd);
    let label_root = git_root.as_deref().unwrap_or(cwd);
    for dir in dirs {
        let Some(path) = agents_md_in_dir(&dir) else {
            continue;
        };
        if already_have(&out, &path) {
            continue;
        }
        let label = if git_root.is_some() {
            posix_rel(label_root, &path)
        } else {
            WORKSPACE_LABEL.to_string()
        };
        out.push((label, path));
    }
    out
}

/// 工作区根 + 本机 `~/.pointer`（生产发现入口）。
pub fn discover_agents_md(workspace_root: &Path) -> Vec<PathBuf> {
    discover_agents_md_chain(Some(workspace_root), pointer_home_dir_no_create().as_deref())
        .into_iter()
        .map(|(_, p)| p)
        .collect()
}

fn read_agents_md_file(path: &Path) -> Result<String> {
    let content = std::fs::read_to_string(path).map_err(|e| anyhow!("读取 {path:?} 失败: {e}"))?;
    Ok(content.trim().to_string())
}

fn merge_labeled_files(files: &[(String, PathBuf)]) -> Result<String> {
    let mut parts = Vec::new();
    for (label, path) in files {
        let body = read_agents_md_file(path)?;
        if body.is_empty() {
            log::info!("agents_md: skip empty file path={}", path.display());
            continue;
        }
        parts.push(format!("#### {label}\n{body}"));
    }
    Ok(parts.join("\n\n---\n\n"))
}

/// 读取全局 + 工作区根并合并。任一层缺失则跳过该层。
pub fn read_merged_agents_md(workspace_root: &Path) -> Result<String> {
    let files =
        discover_agents_md_chain(Some(workspace_root), pointer_home_dir_no_create().as_deref());
    merge_labeled_files(&files)
}

fn inject_user_block(tail: &mut Vec<ChatMessage>, content: String) {
    tail.push(ChatMessage {
        id: new_extension_message_id("agents_md"),
        role: Role::User,
        content: format!("【工程指令（AGENTS.md）】\n{content}"),
        status: "done".into(),
        created_at: now_ms(),
        tool_calls: None,
        tool_call_id: None,
        error_message: None,
        reasoning: None,
        thoughts: None,
        headline: None,
        raw_content: None,
        tool_raw_output: None,
        agent_id: None,
        agent_instance_id: None,
        agent_name: None,
        agent_trace: None,
        image_slot_labels: None,
        images_base64: None,
        computer_round_screen_rel_path: None,
        ui_bindings: None,
        context_state: None,
        attachments: None,
        anchor_message_id: None,
        trace_id: None,
        task_id: None,
        spawn_depth: None,
    });
}

/// 将 AGENTS.md 注入每轮 Prompt 尾部的 hook（单例）。
pub struct AgentsMdInjectHook {
    /// 测试注入假 `~/.pointer`；`None` 表示运行时解析用户主目录。
    pointer_home: Option<PathBuf>,
}

impl Default for AgentsMdInjectHook {
    fn default() -> Self {
        Self { pointer_home: None }
    }
}

impl AgentsMdInjectHook {
    fn resolved_pointer_home(&self) -> Option<PathBuf> {
        if self.pointer_home.is_some() {
            return self.pointer_home.clone();
        }
        match dirs::home_dir() {
            Some(h) => Some(h.join(".pointer")),
            None => {
                log::warn!("agents_md: 无法解析用户主目录，跳过 ~/.pointer/AGENTS.md");
                None
            }
        }
    }
}

#[async_trait]
impl MessageLoopPromptsAfterHook for AgentsMdInjectHook {
    fn override_key(&self) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed(HOOK_KEY)
    }

    fn sort_key(&self) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed(SORT_KEY)
    }

    async fn execute(&self, ctx: &mut MessageLoopPromptsAfterContext<'_>) -> Result<()> {
        let started = Instant::now();
        let raw = ctx.workspace_root.trim();
        let pointer_home = self.resolved_pointer_home();

        let workspace_root = if raw.is_empty() {
            log::info!(
                "agents_md: skip empty workspace conversation_id={}",
                ctx.conversation_id
            );
            None
        } else {
            let root = PathBuf::from(raw);
            if !root.is_dir() {
                log::warn!(
                    "agents_md: workspace is not a directory conversation_id={} root={}",
                    ctx.conversation_id,
                    root.display()
                );
                None
            } else {
                Some(root)
            }
        };

        let files = discover_agents_md_chain(
            workspace_root.as_deref(),
            pointer_home.as_deref(),
        );
        let mut injected = false;
        match merge_labeled_files(&files) {
            Ok(content) if content.is_empty() => {}
            Ok(content) => {
                inject_user_block(ctx.injected_tail, content);
                injected = true;
            }
            Err(err) => {
                log::warn!(
                    "agents_md: 读取失败 conversation_id={} workspace={} elapsed_ms={} files={} err={err:#}",
                    ctx.conversation_id,
                    workspace_root
                        .as_deref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                    started.elapsed().as_millis(),
                    files.len()
                );
                return Ok(());
            }
        }
        let git_root = workspace_root
            .as_ref()
            .and_then(|p| p.canonicalize().ok())
            .and_then(|c| find_git_root(&c));
        let elapsed_ms = started.elapsed().as_millis();
        let labels: Vec<&str> = files.iter().map(|(l, _)| l.as_str()).collect();
        let msg = format!(
            "agents_md: load conversation_id={} workspace={} git_root={} elapsed_ms={} files={} labels={labels:?} injected={}",
            ctx.conversation_id,
            workspace_root
                .as_deref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            git_root
                .as_deref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            elapsed_ms,
            files.len(),
            injected
        );
        if elapsed_ms >= 1000 {
            log::warn!("{msg}");
        } else {
            log::info!("{msg}");
        }
        Ok(())
    }
}

/// 注册 AGENTS.md 注入到 ExtensionRegistry（AppState 构建时调用）。
pub fn register_agents_md_hook(extensions: &crate::extensions::ExtensionRegistry) {
    log::info!(
        "agents_md: registered hook (global ~/.pointer/AGENTS.md + git-root-to-workspace chain)"
    );
    extensions.register_message_loop_prompts_after(std::sync::Arc::new(AgentsMdInjectHook::default()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn hook_ctx<'a>(
        computer: &'a crate::agents::computer::ComputerState,
        base: &'a [crate::models::ChatMessage],
        tail: &'a mut Vec<ChatMessage>,
        workspace: &'a str,
    ) -> MessageLoopPromptsAfterContext<'a> {
        MessageLoopPromptsAfterContext {
            computer_state: computer,
            lead_agent_profile: crate::agents::AgentProfile::General,
            base_messages: base,
            injected_tail: tail,
            conversation_id: "test",
            stream: None,
            round_assistant_message_id: None,
            round_screen_dump_prefix: None,
            task_board_store: std::sync::Arc::new(crate::task_board::TaskBoardStore::new()),
            task_board_store_key: "test",
            user_dynamic_inject_enabled: true,
            workspace_root: workspace,
        }
    }

    fn init_git(dir: &Path) {
        fs::create_dir_all(dir.join(".git")).unwrap();
    }

    #[test]
    fn discovers_global_then_workspace_root_only() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("pointer-home");
        let root = tmp.path().join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::write(home.join("AGENTS.md"), "global rules\n").unwrap();
        fs::write(root.join("AGENTS.md"), "root rules\n").unwrap();
        fs::write(root.join("src/AGENTS.md"), "src rules\n").unwrap();

        let files = discover_agents_md_chain(Some(&root), Some(&home));
        let labels: Vec<&str> = files.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, vec![GLOBAL_LABEL, WORKSPACE_LABEL]);
        let merged = merge_labeled_files(&files).unwrap();
        let g = merged.find("global rules").expect("global");
        let r = merged.find("root rules").expect("root");
        assert!(g < r);
        assert!(!merged.contains("src rules"));
    }

    #[test]
    fn workspace_only_when_global_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("pointer-home");
        let root = tmp.path().join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("AGENTS.md"), "root rules\n").unwrap();
        let files = discover_agents_md_chain(Some(&root), Some(&home));
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, WORKSPACE_LABEL);
    }

    #[test]
    fn skips_duplicate_when_workspace_is_pointer_home() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("pointer-home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("AGENTS.md"), "once\n").unwrap();
        let files = discover_agents_md_chain(Some(&home), Some(&home));
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, GLOBAL_LABEL);
    }

    #[test]
    fn concatenates_git_root_to_workspace_not_siblings() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("pointer-home");
        let repo = tmp.path().join("repo");
        let web = repo.join("packages/web");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&web).unwrap();
        fs::create_dir_all(repo.join("packages/api")).unwrap();
        init_git(&repo);
        fs::write(home.join("AGENTS.md"), "global rules\n").unwrap();
        fs::write(repo.join("AGENTS.md"), "repo rules\n").unwrap();
        fs::write(repo.join("packages/AGENTS.md"), "packages rules\n").unwrap();
        fs::write(web.join("AGENTS.md"), "web rules\n").unwrap();
        fs::write(repo.join("packages/api/AGENTS.md"), "api rules\n").unwrap();

        let files = discover_agents_md_chain(Some(&web), Some(&home));
        let labels: Vec<&str> = files.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                GLOBAL_LABEL,
                "AGENTS.md",
                "packages/AGENTS.md",
                "packages/web/AGENTS.md",
            ]
        );
        let merged = merge_labeled_files(&files).unwrap();
        assert!(merged.find("global rules").unwrap() < merged.find("repo rules").unwrap());
        assert!(merged.find("repo rules").unwrap() < merged.find("packages rules").unwrap());
        assert!(merged.find("packages rules").unwrap() < merged.find("web rules").unwrap());
        assert!(!merged.contains("api rules"));
    }

    #[test]
    fn git_worktree_file_counts_as_root() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let nested = repo.join("pkg");
        fs::create_dir_all(&nested).unwrap();
        fs::write(repo.join(".git"), "gitdir: /tmp/fake.git\n").unwrap();
        fs::write(repo.join("AGENTS.md"), "repo\n").unwrap();
        fs::write(nested.join("AGENTS.md"), "pkg\n").unwrap();
        let labels: Vec<String> = discover_agents_md_chain(Some(&nested), None)
            .into_iter()
            .map(|(l, _)| l)
            .collect();
        assert_eq!(labels, vec!["AGENTS.md".to_string(), "pkg/AGENTS.md".to_string()]);
    }

    #[test]
    fn discover_empty_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("pointer-home");
        fs::create_dir_all(&home).unwrap();
        assert!(discover_agents_md_chain(Some(tmp.path()), Some(&home)).is_empty());
    }

    #[test]
    fn skips_directory_named_agents_md() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("AGENTS.md")).unwrap();
        assert!(discover_agents_md_chain(Some(root), None).is_empty());
    }

    #[tokio::test]
    async fn hook_injects_global_and_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("pointer-home");
        let root = tmp.path().join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(home.join("AGENTS.md"), "global rules\n").unwrap();
        fs::write(root.join("AGENTS.md"), "root rules\n").unwrap();
        fs::write(root.join("src/AGENTS.md"), "src rules\n").unwrap();

        let root_s = root.to_str().expect("utf-8 temp path");
        let computer =
            crate::agents::computer::ComputerState::with_annotate_url("http://127.0.0.1:9");
        let base: &[crate::models::ChatMessage] = &[];
        let mut tail = Vec::new();
        let mut ctx = hook_ctx(&computer, base, &mut tail, root_s);
        let hook = AgentsMdInjectHook {
            pointer_home: Some(home),
        };
        hook.execute(&mut ctx).await.unwrap();
        assert_eq!(tail.len(), 1);
        assert!(tail[0].content.contains("工程指令"));
        assert!(tail[0].content.contains("global rules"));
        assert!(tail[0].content.contains("root rules"));
        assert!(!tail[0].content.contains("src rules"));
    }

    #[tokio::test]
    async fn hook_injects_global_when_workspace_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("pointer-home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("AGENTS.md"), "global only\n").unwrap();
        let computer =
            crate::agents::computer::ComputerState::with_annotate_url("http://127.0.0.1:9");
        let base: &[crate::models::ChatMessage] = &[];
        let mut tail = Vec::new();
        let mut ctx = hook_ctx(&computer, base, &mut tail, "");
        let hook = AgentsMdInjectHook {
            pointer_home: Some(home),
        };
        hook.execute(&mut ctx).await.unwrap();
        assert_eq!(tail.len(), 1);
        assert!(tail[0].content.contains("global only"));
    }

    #[tokio::test]
    async fn hook_noop_without_agents_md() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("pointer-home");
        fs::create_dir_all(&home).unwrap();
        let root_s = tmp.path().to_str().expect("utf-8 temp path");
        let computer =
            crate::agents::computer::ComputerState::with_annotate_url("http://127.0.0.1:9");
        let base: &[crate::models::ChatMessage] = &[];
        let mut tail = Vec::new();
        let mut ctx = hook_ctx(&computer, base, &mut tail, root_s);
        let hook = AgentsMdInjectHook {
            pointer_home: Some(home),
        };
        hook.execute(&mut ctx).await.unwrap();
        assert!(tail.is_empty());
    }

    #[tokio::test]
    async fn hook_noop_empty_files() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("pointer-home");
        let root = tmp.path().join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&root).unwrap();
        fs::write(home.join("AGENTS.md"), "  \n").unwrap();
        fs::write(root.join("AGENTS.md"), "  \n").unwrap();
        let root_s = root.to_str().expect("utf-8 temp path");
        let computer =
            crate::agents::computer::ComputerState::with_annotate_url("http://127.0.0.1:9");
        let base: &[crate::models::ChatMessage] = &[];
        let mut tail = Vec::new();
        let mut ctx = hook_ctx(&computer, base, &mut tail, root_s);
        let hook = AgentsMdInjectHook {
            pointer_home: Some(home),
        };
        hook.execute(&mut ctx).await.unwrap();
        assert!(tail.is_empty());
    }

    #[test]
    fn skips_unix_filesystem_root() {
        if !cfg!(unix) {
            return;
        }
        assert!(discover_agents_md_chain(Some(Path::new("/")), None).is_empty());
    }
}
