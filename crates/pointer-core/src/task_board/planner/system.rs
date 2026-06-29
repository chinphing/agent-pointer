//! Planner system prompt assembly.

use crate::models::SystemPromptSections;
use crate::task_board::snapshot::markdown_runtime_block_for_inject;
use crate::task_board::{BoardDocument, WorkItemStore};

const COMPUTER_MD: &str = include_str!("prompts/computer.md");
const TASK_BOARD_INIT_MD: &str = include_str!("prompts/task_board_init.md");
const TASK_BOARD_REPLACE_MD: &str = include_str!("prompts/task_board_replace.md");
const TASK_BOARD_ABANDON_MD: &str = include_str!("prompts/task_board_abandon.md");

pub struct PlannerSystemInput<'a> {
    pub doc: &'a BoardDocument,
    pub store_key: &'a str,
    pub work_items: Option<&'a WorkItemStore>,
    pub workspace_root: &'a str,
    pub today_line: &'a str,
    /// Sub-agent delegated task blocks (`## Assigned task`, spawn depth, …) — execution-aligned.
    pub system_dynamic: &'a [String],
}

pub fn build_planner_system(input: PlannerSystemInput<'_>) -> SystemPromptSections {
    let board_block = if input.doc.meta.goal.trim().is_empty() && input.doc.global_milestones.is_empty() {
        "[CURRENT_TASK_BOARD]\n(empty — no goal or milestones yet)".to_string()
    } else {
        markdown_runtime_block_for_inject(input.doc, input.store_key, input.work_items)
    };
    let env = format!(
        "[Environment]\n{}\nworkspace_root: {}",
        input.today_line.trim(),
        input.workspace_root.trim()
    );
    let tools_doc = format!(
        "## Tools\n\n### task_board_init\n{TASK_BOARD_INIT_MD}\n\n### task_board_replace\n{TASK_BOARD_REPLACE_MD}\n\n### task_board_abandon\n{TASK_BOARD_ABANDON_MD}"
    );
    let body = format!(
        "{COMPUTER_MD}\n\n{board_block}\n\n{env}\n\n{tools_doc}"
    );
    SystemPromptSections {
        cacheable: vec![body],
        dynamic: input.system_dynamic.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_board::model::BoardDocument;
    use crate::task_board::TaskBoardStore;

    #[test]
    fn empty_board_uses_placeholder_block() {
        let doc = BoardDocument::empty_for_store_key("conv1");
        let store = TaskBoardStore::new();
        let sections = build_planner_system(PlannerSystemInput {
            doc: &doc,
            store_key: "conv1",
            work_items: Some(store.work_items.as_ref()),
            workspace_root: "/tmp/ws",
            today_line: "[Environment] Today is Monday, 2026-01-01.",
            system_dynamic: &[],
        });
        let body = sections.cacheable.join("\n\n");
        assert!(body.contains("(empty — no goal or milestones yet)"));
        assert!(body.contains("workspace_root: /tmp/ws"));
        assert!(body.contains("### task_board_init"));
        assert!(body.contains("### task_board_replace"));
        assert!(!body.contains("### session_search"));
        assert!(!body.contains("### web_search"));
    }

    #[test]
    fn populated_board_includes_runtime_block() {
        let store = TaskBoardStore::new();
        let key = "conv2";
        store
            .apply(
                key,
                "init",
                &serde_json::json!({
                    "goal": "open apps",
                    "items": [{"id": "s1", "title": "Step", "status": "pending"}]
                }),
            )
            .expect("init");
        let doc = store.document(key);
        let sections = build_planner_system(PlannerSystemInput {
            doc: &doc,
            store_key: key,
            work_items: Some(store.work_items.as_ref()),
            workspace_root: "",
            today_line: "[Environment] Today is Tuesday.",
            system_dynamic: &[],
        });
        let body = sections.cacheable.join("\n\n");
        assert!(body.contains("[CURRENT_TASK_BOARD]"));
        assert!(body.contains("open apps"));
        assert!(!body.contains("(empty — no goal or milestones yet)"));
    }

    #[test]
    fn sub_agent_task_blocks_go_in_system_dynamic() {
        let doc = BoardDocument::empty_for_store_key("child");
        let store = TaskBoardStore::new();
        let dynamic = vec!["## Assigned task\n\n```\nopen ten apps\n```".to_string()];
        let sections = build_planner_system(PlannerSystemInput {
            doc: &doc,
            store_key: "child",
            work_items: Some(store.work_items.as_ref()),
            workspace_root: "/ws",
            today_line: "[Environment] Today is Wednesday.",
            system_dynamic: &dynamic,
        });
        assert!(sections.cacheable.iter().any(|s| s.contains("Task Board Planner")));
        assert_eq!(sections.dynamic.len(), 1);
        assert!(sections.dynamic[0].contains("open ten apps"));
    }
}
