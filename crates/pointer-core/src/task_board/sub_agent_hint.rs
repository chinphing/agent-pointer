//! Optional system hint when a sub-agent may use `task_board` but has an empty local board.

use crate::agents::AgentProfile;
use super::store::TaskBoardStore;

const HINT_BLOCK: &str = "\
[TASK_BOARD_HINT]
Your local task board is empty.
For multi-step subtasks, call **`task_board`** with **`method`: `init`** early.
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
If this is multi-step work, initialize early with **`task_board`** and **`method`: `init`**.
Use 3-6 concise milestones for normal work.
For exhaustive matrix/combinational goals,
keep grouped milestones by interaction form
instead of enumerating every case.
Single-step work may skip the board.
For turns that also emit **verify:report**:
- first board-init round may omit report;
- after init, run `verify.report` first, then `task_board` with **`method`: `patch`**.
Keep task board text compact to reduce prompt token cost.
";

const CODER_MAIN_SESSION_HINT_BLOCK: &str = "\
[TASK_BOARD_HINT]
Your task board is empty.
For **any behavior change** (logic, API, state, errors, constants),
call **`task_board`** with **`method`: `init`** in **Plan**
(after Explore + Impact scan, before heavy edits).
Use **3–6** rows — include **Impact scan**, **Implement**, and **Unit tests**.
Each row needs a short **`verification`** line (grep/read proof, test command, or file pair).
**Cadence:** when a milestone starts or finishes, call **`task_board`**
with **`method`: `patch`** in the **same turn** — do not wait until Deliver only.
Skip **`init`** only for comment/format/rename-only edits with no behavior change.
When **all** rows are **`done`** or **`cancelled`**, call **`method`: `finalize`** before delivery.
Complete **Responsibility audit** (AGENT step 7) before finalize when logic changed.
Keep row text compact to reduce prompt token cost.
";

/// System slice appended at sub-agent session start when `task_board` is allowed and the child board is empty.
pub fn sub_agent_task_board_init_hint(
    store: &TaskBoardStore,
    child_store_key: &str,
    allowed_tools: &[String],
) -> Option<String> {
    if !allowed_tools
        .iter()
        .any(|t| crate::tools::registry_tool_base_name(t) == "task_board")
    {
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
    if !matches!(profile, AgentProfile::Computer | AgentProfile::Coder) {
        return None;
    }
    let doc = store.document(store_key);
    if !doc.board_is_empty() || !doc.meta.goal.is_empty() {
        return None;
    }
    let hint = match profile {
        AgentProfile::Coder => CODER_MAIN_SESSION_HINT_BLOCK,
        _ => MAIN_SESSION_HINT_BLOCK,
    };
    Some(hint.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentProfile;

    #[test]
    fn main_agent_hint_for_coder_when_board_empty() {
        let store = TaskBoardStore::new();
        let hint = main_agent_task_board_init_hint(&store, "conv-1", &AgentProfile::Coder);
        let text = hint.expect("coder hint");
        assert!(text.contains("TASK_BOARD_HINT"));
        assert!(text.contains("method"));
    }

    #[test]
    fn main_agent_hint_skips_when_board_has_goal() {
        let store = TaskBoardStore::new();
        store
            .apply(
                "conv-1",
                "init",
                &serde_json::json!({ "goal": "g", "items": [] }),
            )
            .expect("init");
        assert!(main_agent_task_board_init_hint(&store, "conv-1", &AgentProfile::Coder).is_none());
    }
}
