//! Optional system hint when a sub-agent may use `task_board` but has an empty local board.

use crate::agents::AgentProfile;
use super::store::TaskBoardStore;

const HINT_BLOCK: &str = "\
[TASK_BOARD_HINT]
Your local task board is empty.
For multi-step subtasks, call **`task_board_init`** early.
Use 3-6 **local_*** steps in **`global_milestones`** for normal work.
For each active step, keep:
- `plan`: how to execute;
- `done_when`: milestone outcome acceptance criteria;
- `remark`: short outcome note when marking `done`.
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
            "Complexity gate (computer): initialize task_board when expected operation steps > 3,
or when the user task has more than 5 similar repetitive operations (enumerated targets or cycles).
For <=3 deterministic steps, skip init by default.
Escalate to init if retries or branching make the flow multi-step.",
        ),
        _ => None,
    }
}

fn main_agent_task_board_hint(profile: &AgentProfile) -> Option<String> {
    let gate = main_agent_complexity_gate(profile)?;
    let verify_order = if matches!(profile, AgentProfile::Computer) {
        let av = crate::agents::computer::tool_names::ACTION_VERIFY;
        format!(
            "For turns that also emit **{av}** (step check, not milestone):
- first board-init round may omit report;
- after init, run `{av}` first, then `task_board_patch`.
"
        )
    } else {
        String::new()
    };
    let profile_rows = if matches!(profile, AgentProfile::Coder) {
        "Use **3-6** milestones in **`global_milestones`** when initialized, including **Recon**, **Implement**, and **Unit tests**.
Each milestone: `plan`, `done_when`; optional `remark` when marking `done`.
"
    } else if matches!(profile, AgentProfile::Computer) {
        "Use **3-6** milestones for normal GUI work (Type1).
For enumerated work (>5 similar items), use Type2: `g_plan`/`g_exec`/`g_deliver` + `item_milestones` + `work_items` on init.
During `g_exec`: **`task_board_patch` every turn** that completes a milestone step (`milestones` done/failed + remark on last row).
Host advances the work queue — do not send `work_item_delta`.
When inject shows `exec_met: true`, patch `g_exec` to `done`, then handle `g_deliver` + export.
Set **`expected_total`** when the exhaustive count is known.
"
    } else {
        "Use **3-6** concise milestones in **`global_milestones`** for normal multi-step work.
"
    };
    Some(format!(
        "[TASK_BOARD_HINT]
Your task board is empty.
{gate}
If gate is met, initialize with **`task_board_init`**.
{profile_rows}For exhaustive matrix/combinational goals, keep grouped milestones by interaction form (never one milestone for all atomic cases).
Patch each substantive step during execution, not only at final delivery.
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
    if !allowed_tools.iter().any(|t| {
        crate::tools::registry_tool_in_allow_list(&["task_board".to_string()], t)
    }) {
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
    if !doc.global_milestones.is_empty() || !doc.meta.goal.is_empty() {
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
