//! Optional system hint when a sub-agent may use `task_board` but has an empty local board.

use crate::agents::AgentProfile;
use super::store::TaskBoardStore;

const HINT_BLOCK: &str = "\
[TASK_BOARD_HINT]
Your local task board is empty.
For multi-step subtasks, call **`task_board_init`** early.
Use 3-6 **local_*** steps for normal work.
For each active step, keep:
- `plan`: how to execute;
- `checkpoint`: coarse position only (update rarely);
- `validate_requirement`: milestone outcome acceptance criteria;
- `validate_results`: append-only outcome evidence (markdown snippets).
For extraction milestones, use `extract_requirement` / `extract_results` (append-only).
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
        "For turns that also emit **verify:report** (step check, not milestone):
- first board-init round may omit report;
- after init, run `verify.report` first, then `task_board_patch`.
"
    } else {
        ""
    };
    let coder_rows = if matches!(profile, AgentProfile::Coder) {
        "Use **3-6** milestones in **`items`** when initialized, including **Impact scan**, **Implement**, and **Unit tests**.
Each milestone: `plan`, `validate_requirement`, append `validate_results` when evidence exists.
"
    } else {
        "Use **3-6** concise milestones in **`items`** for normal multi-step work.
"
    };
    Some(format!(
        "[TASK_BOARD_HINT]
Your task board is empty.
{gate}
If gate is met, initialize with **`task_board_init`**.
{coder_rows}For exhaustive matrix/combinational goals, keep grouped milestones by interaction form.
When milestone status changes, patch in the same turn; do not defer updates to final delivery.
When all milestones are `done` or `cancelled`, call `finalize` before final delivery.
{verify_order}User delivery goes in assistant **content**, not board prose fields.
Keep board text compact."
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
    if !doc.board.is_empty() || !doc.meta.goal.is_empty() {
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
        assert!(text.contains("task_board_init"));
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
