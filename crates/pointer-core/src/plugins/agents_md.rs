//! `AGENTS.md` 工程指令发现链（设计稿 §4.4 / §4.2）。
//!
//! 工作区根 + 子目录嵌套扫描（子目录优先），合并后注入
//! [`ExtensionRegistry::MessageLoopPromptsAfter`]（作为工程指令的 Prompt 扩展点）。
//!
//! 规则：
//! - 只扫描 `<workspace>/AGENTS.md` 及子目录中的 `AGENTS.md`（嵌套，子目录优先）；
//! - 跳过隐藏目录、`node_modules`、`target`、`.git` 等大型/生成目录；
//! - 合并顺序：深度浅 → 深（子目录覆盖/追加在根之后），输出为单个 user 注入块；
//! - 发现 ≠ 执行：仅读取文本注入 Prompt，不改变任何运行时行为。

use crate::extensions::{
    new_extension_message_id, now_ms, MessageLoopPromptsAfterContext, MessageLoopPromptsAfterHook,
};
use crate::models::{ChatMessage, Role};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};

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

/// 发现工作区根 + 子目录中的 AGENTS.md，返回按深度升序（根 → 深层）的路径列表。
/// 子目录优先体现在合并阶段：深层内容追加在根内容之后（子目录覆盖/追加）。
pub fn discover_agents_md(workspace_root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect_agents_md(workspace_root, workspace_root, 0, &mut found);
    found.sort_by_key(|p| p.components().count());
    found
}

fn collect_agents_md(base: &Path, dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 0 {
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if SKIP_DIRS.iter().any(|s| *s == name) || name.starts_with('.') {
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
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_agents_md(base, &path, depth + 1, out);
        }
    }
}

/// 读取并合并所有 AGENTS.md：每份文件输出「路径 + 内容」，根在前、子目录在后。
pub fn read_merged_agents_md(workspace_root: &Path) -> Result<String> {
    let files = discover_agents_md(workspace_root);
    if files.is_empty() {
        return Ok(String::new());
    }
    let mut parts = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(workspace_root)
            .unwrap_or(&path)
            .to_string_lossy();
        let content =
            std::fs::read_to_string(&path).map_err(|e| anyhow!("读取 {path:?} 失败: {e}"))?;
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
pub struct AgentsMdInjectHook {
    workspace_root: PathBuf,
}

impl AgentsMdInjectHook {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
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
        let content = match read_merged_agents_md(&self.workspace_root) {
            Ok(c) => c,
            Err(err) => {
                log::warn!("agents_md: 读取失败: {err:#}");
                return Ok(());
            }
        };
        if content.is_empty() {
            return Ok(());
        }
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
        Ok(())
    }
}

/// 注册 AGENTS.md 发现链到 ExtensionRegistry（AppState 构建时调用）。
pub fn register_agents_md_hook(
    extensions: &crate::extensions::ExtensionRegistry,
    workspace_root: PathBuf,
) {
    extensions.register_message_loop_prompts_after(std::sync::Arc::new(AgentsMdInjectHook::new(
        workspace_root,
    )));
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
        };
        let hook = AgentsMdInjectHook::new(root.to_path_buf());
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
        };
        let hook = AgentsMdInjectHook::new(root.to_path_buf());
        hook.execute(&mut ctx).await.unwrap();
        assert!(tail.is_empty());
    }
}
