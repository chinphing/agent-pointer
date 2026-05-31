//! Optional system hint when a sub-agent may use `task_board` but has an empty local board.

use crate::agents::AgentProfile;
use super::store::TaskBoardStore;

const HINT_BLOCK: &str = "\
[TASK_BOARD_HINT]
Your local task board is empty.
For multi-step subtasks, call **`task_board`** with **`method`: `init`** early.
Use 3-6 **local_*** steps for normal work.
For each active step, keep:
- `details`: plan + key implementation notes;
- `progress`: current partial progress;
- `validate`: final acceptance check only.
For exhaustive matrix/combinational goals,
keep grouped milestones by interaction form
instead of enumerating every case.
Single-step subtasks may skip the board.
Read **[TASK_BOARD_PARENT]** for the parent goal and milestone; do not patch parent rows.
";

fn main_agent_complexity_gate(profile: &AgentProfile) -> Option<&'static str> {
    match profile {
        AgentProfile::Coder => Some(
            "Complexity gate (coder): initialize task_board only when expected scope is >=2 files or cross-module.
For single-file / narrow changes, skip init by default.
Escalate to init if scope expands during exploration.",
        ),
        AgentProfile::Computer => Some(
            "Complexity gate (computer): initialize task_board only when expected operation steps > 3.
For <=3 deterministic steps, skip init by default.
Escalate to init if retries or branching make the flow multi-step.",
        ),
        _ => None,
    }
}

fn main_agent_task_board_hint(profile: &AgentProfile) -> Option<String> {
    let gate = main_agent_complexity_gate(profile)?;
    let verify_order = if matches!(profile, AgentProfile::Computer) {
        "For turns that also emit **verify:report**:
- first board-init round may omit report;
- after init, run `verify.report` first, then `task_board` with **`method`: `patch`**.
"
    } else {
        ""
    };
    let coder_rows = if matches!(profile, AgentProfile::Coder) {
        "Use **3-6** rows when initialized, including **Impact scan**, **Implement**, and **Unit tests**.
Each row keeps `details`, `progress`, and final `validate`.
"
    } else {
        "Use **3-6** concise milestones for normal multi-step work.
"
    };
    Some(format!(
        "[TASK_BOARD_HINT]
Your task board is empty.
{gate}
If gate is met, initialize with **`task_board`** and **`method`: `init`**.
{coder_rows}For exhaustive matrix/combinational goals, keep grouped milestones by interaction form.
When milestone status changes, patch in the same turn; do not defer updates to final delivery.
When all rows are `done` or `cancelled`, call `finalize` before final delivery.
{verify_order}Use `details` for execution details and key points.
Use `validate` only for final acceptance evidence.
Keep task board text compact to reduce prompt token cost."
    ))
}

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
    main_agent_task_board_hint(profile)
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
