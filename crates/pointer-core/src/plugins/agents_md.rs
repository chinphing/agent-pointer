//! `AGENTS.md` 工程指令发现链（设计稿 §4.4 / §4.2）。
//!
//! 工作区根 + 子目录嵌套扫描（子目录优先），合并后注入
//! [`ExtensionRegistry::MessageLoopPromptsAfter`]（作为工程指令的 Prompt 扩展点）。
//!
//! 规则：
//! - 每轮使用会话工作区（`workspace_root`），不使用进程 cwd；
//! - 只扫描 `<workspace>/AGENTS.md` 及子目录中的 `AGENTS.md`（嵌套，子目录优先）；
//! - 跳过隐藏目录、`node_modules`、`target`、`.git` 等大型/生成目录；
//! - 不扫描文件系统根（`/` / `C:\`），也不进入 `Volumes` / `mnt` 等挂载点目录；
//! - 合并顺序：深度浅 → 深（子目录覆盖/追加在根之后），输出为单个 user 注入块；
//! - 发现 ≠ 执行：仅读取文本注入 Prompt，不改变任何运行时行为。

use crate::extensions::{
    new_extension_message_id, now_ms, MessageLoopPromptsAfterContext, MessageLoopPromptsAfterHook,
};
use crate::models::{ChatMessage, Role};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// AGENTS.md 文件名。
pub const AGENTS_MD: &str = "AGENTS.md";
/// Extension hook 的 override_key（固定单例，重复注册替换）。
const HOOK_KEY: &str = "workspace_agents_md";
/// 注入排序（在 task board 等动态注入之前、内置钩子之后）。
const SORT_KEY: &str = "_60_agents_md";

/// 跳过的大型 / 生成目录（与 repo 内 `ignore` 默认习惯对齐）。
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".cache",
];

/// Mount / share roots. Walking these from `/` triggers macOS “network volume”
/// TCC prompts and multi-second stalls on SMB/AFP/NFS.
const SKIP_MOUNT_DIR_NAMES: &[&str] = &[
    "Volumes",
    "Network",
    "mnt",
    "media",
    "net",
    "proc",
    "sys",
    "dev",
];

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

/// 发现工作区根 + 子目录中的 AGENTS.md，返回按深度升序（根 → 深层）的路径列表。
/// 子目录优先体现在合并阶段：深层内容追加在根内容之后（子目录覆盖/追加）。
pub fn discover_agents_md(workspace_root: &Path) -> Vec<PathBuf> {
    discover_agents_md_stats(workspace_root).0
}

fn discover_agents_md_stats(workspace_root: &Path) -> (Vec<PathBuf>, usize) {
    if is_filesystem_root(workspace_root) {
        log::warn!(
            "agents_md: skip filesystem root scan root={}",
            workspace_root.display()
        );
        return (Vec::new(), 0);
    }
    let mut found = Vec::new();
    let mut dirs_visited = 0usize;
    collect_agents_md(
        workspace_root,
        workspace_root,
        0,
        &mut found,
        &mut dirs_visited,
    );
    found.sort_by_key(|p| p.components().count());
    (found, dirs_visited)
}

fn collect_agents_md(
    base: &Path,
    dir: &Path,
    depth: usize,
    out: &mut Vec<PathBuf>,
    dirs_visited: &mut usize,
) {
    *dirs_visited += 1;
    if depth > 0 {
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if SKIP_DIRS.iter().any(|s| *s == name)
            || SKIP_MOUNT_DIR_NAMES.iter().any(|s| *s == name)
            || name.starts_with('.')
        {
            return;
        }
    }
    let candidate = dir.join(AGENTS_MD);
    if candidate.is_file() {
        out.push(candidate);
    }
    if depth >= 6 {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            if dir == base {
                log::warn!(
                    "agents_md: read_dir failed root={} err={e}",
                    dir.display()
                );
            } else {
                log::debug!("agents_md: read_dir skipped path={} err={e}", dir.display());
            }
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_agents_md(base, &path, depth + 1, out, dirs_visited);
        }
    }
}

/// 读取并合并所有 AGENTS.md：每份文件输出「路径 + 内容」，根在前、子目录在后。
pub fn read_merged_agents_md(workspace_root: &Path) -> Result<String> {
    let files = discover_agents_md(workspace_root);
    read_merged_from_files(workspace_root, &files)
}

fn read_merged_from_files(workspace_root: &Path, files: &[PathBuf]) -> Result<String> {
    if files.is_empty() {
        return Ok(String::new());
    }
    let mut parts = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(workspace_root)
            .unwrap_or(path)
            .to_string_lossy();
        let content =
            std::fs::read_to_string(path).map_err(|e| anyhow!("读取 {path:?} 失败: {e}"))?;
        let trimmed = content.trim();
        if trimmed.is_empty() {
            continue;
        }
        parts.push(format!("#### {rel}\n{trimmed}"));
    }
    if parts.is_empty() {
        return Ok(String::new());
    }
    Ok(parts.join("\n\n---\n\n"))
}

/// 将合并后的 AGENTS.md 注入每轮 Prompt 尾部的 hook（单例）。
pub struct AgentsMdInjectHook;

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
        if raw.is_empty() {
            log::info!(
                "agents_md: skip empty workspace conversation_id={}",
                ctx.conversation_id
            );
            return Ok(());
        }
        let workspace_root = PathBuf::from(raw);
        if !workspace_root.is_dir() {
            log::warn!(
                "agents_md: workspace is not a directory conversation_id={} root={}",
                ctx.conversation_id,
                workspace_root.display()
            );
            return Ok(());
        }
        let (files, dirs_visited) = discover_agents_md_stats(&workspace_root);
        let mut injected = false;
        let content = match read_merged_from_files(&workspace_root, &files) {
            Ok(c) => c,
            Err(err) => {
                log::warn!(
                    "agents_md: 读取失败 conversation_id={} root={} elapsed_ms={} dirs_visited={} files={} err={err:#}",
                    ctx.conversation_id,
                    workspace_root.display(),
                    started.elapsed().as_millis(),
                    dirs_visited,
                    files.len()
                );
                return Ok(());
            }
        };
        if !content.is_empty() {
            ctx.injected_tail.push(ChatMessage {
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
            injected = true;
        }
        let elapsed_ms = started.elapsed().as_millis();
        let preview: Vec<String> = files
            .iter()
            .take(8)
            .map(|p| {
                p.strip_prefix(&workspace_root)
                    .unwrap_or(p)
                    .display()
                    .to_string()
            })
            .collect();
        let msg = format!(
            "agents_md: scan conversation_id={} root={} elapsed_ms={} dirs_visited={} files={} injected={} preview={preview:?}",
            ctx.conversation_id,
            workspace_root.display(),
            elapsed_ms,
            dirs_visited,
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

/// 注册 AGENTS.md 发现链到 ExtensionRegistry（AppState 构建时调用）。
pub fn register_agents_md_hook(extensions: &crate::extensions::ExtensionRegistry) {
    log::info!("agents_md: registered hook (scan conversation workspace_root each round)");
    extensions.register_message_loop_prompts_after(std::sync::Arc::new(AgentsMdInjectHook));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discovers_nested_agents_md() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        fs::create_dir_all(root.join("target/x")).unwrap();
        fs::write(root.join("AGENTS.md"), "root rules\n").unwrap();
        fs::write(root.join("src/AGENTS.md"), "src rules\n").unwrap();
        fs::write(root.join("src/nested/AGENTS.md"), "nested rules\n").unwrap();
        fs::write(root.join("node_modules/pkg/AGENTS.md"), "ignored\n").unwrap();
        fs::write(root.join("target/x/AGENTS.md"), "ignored\n").unwrap();

        let files = discover_agents_md(root);
        let names: Vec<String> = files
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"AGENTS.md".to_string()));
        assert!(names.contains(&"src/AGENTS.md".to_string()));
        assert!(names.contains(&"src/nested/AGENTS.md".to_string()));
        assert!(!names.iter().any(|n| n.contains("node_modules")));
        assert!(!names.iter().any(|n| n.contains("target")));
    }

    #[test]
    fn merges_root_first_then_nested() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("AGENTS.md"), "root rules\n").unwrap();
        fs::write(root.join("src/AGENTS.md"), "src rules\n").unwrap();

        let merged = read_merged_agents_md(root).unwrap();
        let root_pos = merged.find("root rules").expect("root rules present");
        let src_pos = merged.find("src rules").expect("src rules present");
        assert!(root_pos < src_pos, "root should come before nested");
        assert!(merged.contains("AGENTS.md"));
        assert!(merged.contains("src/AGENTS.md"));
    }

    #[tokio::test]
    async fn hook_injects_merged_content() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("AGENTS.md"), "root rules\n").unwrap();
        fs::write(root.join("src/AGENTS.md"), "src rules\n").unwrap();

        let root_s = root.to_str().expect("utf-8 temp path");
        let computer =
            crate::agents::computer::ComputerState::with_annotate_url("http://127.0.0.1:9");
        let base: &[crate::models::ChatMessage] = &[];
        let mut tail = Vec::new();
        let mut ctx = MessageLoopPromptsAfterContext {
            computer_state: &computer,
            lead_agent_profile: crate::agents::AgentProfile::General,
            base_messages: base,
            injected_tail: &mut tail,
            conversation_id: "test",
            stream: None,
            round_assistant_message_id: None,
            round_screen_dump_prefix: None,
            task_board_store: std::sync::Arc::new(crate::task_board::TaskBoardStore::new()),
            task_board_store_key: "test",
            user_dynamic_inject_enabled: true,
            workspace_root: root_s,
        };
        let hook = AgentsMdInjectHook;
        hook.execute(&mut ctx).await.unwrap();
        assert_eq!(tail.len(), 1);
        assert!(tail[0].content.contains("工程指令"));
        assert!(tail[0].content.contains("root rules"));
        assert!(tail[0].content.contains("src rules"));
    }

    #[tokio::test]
    async fn hook_noop_without_agents_md() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let root_s = root.to_str().expect("utf-8 temp path");
        let computer =
            crate::agents::computer::ComputerState::with_annotate_url("http://127.0.0.1:9");
        let base: &[crate::models::ChatMessage] = &[];
        let mut tail = Vec::new();
        let mut ctx = MessageLoopPromptsAfterContext {
            computer_state: &computer,
            lead_agent_profile: crate::agents::AgentProfile::General,
            base_messages: base,
            injected_tail: &mut tail,
            conversation_id: "test",
            stream: None,
            round_assistant_message_id: None,
            round_screen_dump_prefix: None,
            task_board_store: std::sync::Arc::new(crate::task_board::TaskBoardStore::new()),
            task_board_store_key: "test",
            user_dynamic_inject_enabled: true,
            workspace_root: root_s,
        };
        let hook = AgentsMdInjectHook;
        hook.execute(&mut ctx).await.unwrap();
        assert!(tail.is_empty());
    }

    #[test]
    fn skips_volumes_mount_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("Volumes/share")).unwrap();
        fs::write(root.join("AGENTS.md"), "root\n").unwrap();
        fs::write(root.join("Volumes/share/AGENTS.md"), "network\n").unwrap();
        let names: Vec<String> = discover_agents_md(root)
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["AGENTS.md".to_string()]);
    }

    #[test]
    fn skips_unix_filesystem_root() {
        if !cfg!(unix) {
            return;
        }
        let (files, visits) = discover_agents_md_stats(Path::new("/"));
        assert!(files.is_empty());
        assert_eq!(visits, 0);
    }
}
