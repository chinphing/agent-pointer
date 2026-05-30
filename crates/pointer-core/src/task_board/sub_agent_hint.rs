//! Optional system hint when a sub-agent may use `task_board` but has an empty local board.

use crate::agents::AgentProfile;
use super::store::TaskBoardStore;

const HINT_BLOCK: &str = "\
[TASK_BOARD_HINT]
Your local task board is empty.
For multi-step subtasks, call **task_board:init** early.
Use 3-6 **local_*** steps for normal work.
For exhaustive matrix/combinational goals,
keep grouped milestones by interaction form
instead of enumerating every case.
Single-step subtasks may skip the board.
Read **[TASK_BOARD_PARENT]** for the parent goal and milestone; do not patch parent rows.
";

const MAIN_SESSION_HINT_BLOCK: &str = "\
[TASK_BOARD_HINT]
Your task board is empty.
If this is multi-step work, initialize early with **task_board:init**.
Use 3-6 concise milestones for normal work.
For exhaustive matrix/combinational goals,
keep grouped milestones by interaction form
instead of enumerating every case.
Single-step work may skip the board.
For turns that also emit **verify:report**:
- first board-init round may omit report;
- after init, run `verify:report` first, then `task_board:patch`.
Keep task board text compact to reduce prompt token cost.
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

/// System slice appended for lead agents when `task_board` is expected and the board is empty.
pub fn main_agent_task_board_init_hint(
    store: &TaskBoardStore,
    store_key: &str,
    profile: &AgentProfile,
) -> Option<String> {
    if !matches!(profile, AgentProfile::Computer) {
        return None;
    }
    let doc = store.document(store_key);
    if !doc.board_is_empty() || !doc.meta.goal.is_empty() {
        return None;
    }
    Some(MAIN_SESSION_HINT_BLOCK.to_string())
}
