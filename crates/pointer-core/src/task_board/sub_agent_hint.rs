//! Optional system hint when a sub-agent may use `task_board` but has an empty local board.

use super::store::TaskBoardStore;

const HINT_BLOCK: &str = "\
[TASK_BOARD_HINT]
Your local task board is empty. For multi-step subtasks, call **task_board:init** early with 3–6 **local_*** steps (status pending or ready).
Single-step subtasks may skip the board.
Read **[TASK_BOARD_PARENT]** for the parent goal and milestone; do not patch parent rows.
";

/// System slice appended at sub-agent session start when `task_board` is allowed and the child board is empty.
pub fn sub_agent_task_board_init_hint(
    store: &TaskBoardStore,
    child_store_key: &str,
    allowed_tools: &[String],
) -> Option<String> {
    if !allowed_tools.iter().any(|t| t == "task_board") {
        return None;
    }
    let doc = store.document(child_store_key);
    if !doc.board_is_empty() || !doc.meta.goal.is_empty() {
        return None;
    }
    Some(HINT_BLOCK.to_string())
}
