//! Planner system prompt assembly.

use crate::models::SystemPromptSections;
use crate::task_board::snapshot::markdown_runtime_block_for_inject;
use crate::task_board::{BoardDocument, WorkItemStore};

const COMPUTER_MD: &str = include_str!("prompts/computer.md");
const WEB_SEARCH_MD: &str = include_str!("prompts/web_search.md");
const SESSION_SEARCH_MD: &str = include_str!("prompts/session_search.md");
const TASK_BOARD_INIT_MD: &str = include_str!("prompts/task_board_init.md");
const TASK_BOARD_REPLACE_MD: &str = include_str!("prompts/task_board_replace.md");

pub struct PlannerSystemInput<'a> {
    pub doc: &'a BoardDocument,
    pub store_key: &'a str,
    pub work_items: Option<&'a WorkItemStore>,
    pub workspace_root: &'a str,
    pub today_line: &'a str,
}

pub fn build_planner_system(input: PlannerSystemInput<'_>) -> SystemPromptSections {
    let board_block = if input.doc.meta.goal.trim().is_empty() && input.doc.board.is_empty() {
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
        "## Tools\n\n### web_search\n{WEB_SEARCH_MD}\n\n### session_search\n{SESSION_SEARCH_MD}\n\n### task_board_init\n{TASK_BOARD_INIT_MD}\n\n### task_board_replace\n{TASK_BOARD_REPLACE_MD}"
    );
    let body = format!(
        "{COMPUTER_MD}\n\n{board_block}\n\n{env}\n\n{tools_doc}"
    );
    SystemPromptSections::all_cacheable(vec![body])
}
