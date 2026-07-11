//! Optional system hint when a sub-agent may use `task_board` but has an empty local board.

use crate::agents::AgentProfile;
use crate::models::{ChatMessage, Role};
use super::store::TaskBoardStore;

pub const TASK_BOARD_HINT_TAG: &str = "[TASK_BOARD_HINT";

const COMPUTER_LOOP_INIT_ROWS: &str = "\
Use **3–8** milestones for normal GUI work (linear).
For batch / loop work (>5 similar items), use loop shape: `g_plan` / one `wi_*` row per target / `g_deliver`.
Put the full per-item GUI procedure in each item row's **`plan`**.

**Loop init:**
- Known list → inline `work_items[]` or explicit `wi_*` rows in `global_milestones`
- Runtime quota → **`dynamic_quota`** + shared **`loop_item_plan`**; host seeds `wi_1…wi_N` between `g_plan` and `g_deliver`

During exec: patch **`task_board_patch`** on the active **`wi_*`** when that item is terminal (`done` / `failed` + `remark`).
Host auto-advances the next item. Dynamic: set `title` on the active row before executing its `plan`.
When all items are terminal, patch **`g_deliver`** then finalize after user summary.
User switched tasks: **`task_board_abandon`**, then **`task_board_init`** if still multi-step.
";

fn sub_agent_hint_block(profile: &AgentProfile) -> &'static str {
    match profile {
        AgentProfile::Computer => "\
[TASK_BOARD_HINT]
Your local task board is empty.
For multi-step subtasks, call **`task_board_init`** early on the first turn.
Use 3–8 **local_*** steps in **`global_milestones`** for normal work.
For batch / repetitive subtasks, use loop shape (see profile guidance in task_board tool doc).
For each active step, keep:
- `plan`: how to execute;
- `done_when`: milestone outcome acceptance criteria;
- `remark`: short outcome note when marking `done`.
Single-step subtasks may skip the board.
Read **[TASK_BOARD_PARENT]** for the parent goal and milestone; do not patch parent rows.
",
        _ => "\
[TASK_BOARD_HINT]
Your local task board is empty.
For multi-step subtasks, call **`task_board_init`** early.
Use 3–8 **local_*** steps in **`global_milestones`** for normal work.
For each active step, keep:
- `plan`: how to execute;
- `done_when`: milestone outcome acceptance criteria;
- `remark`: short outcome note when marking `done`.
Single-step subtasks may skip the board.
Read **[TASK_BOARD_PARENT]** for the parent goal and milestone; do not patch parent rows.
",
    }
}

fn main_agent_complexity_gate(profile: &AgentProfile) -> Option<&'static str> {
    match profile {
        AgentProfile::Coder => Some(
            "Complexity gate (coder): initialize task_board only when expected scope is >=2 files or cross-module.
For single-file / narrow changes, skip init by default.
Escalate to init if explore or tracing reveals wider scope.",
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
        "Host runs verify after each desktop tool; patch **`wi_*`** when the item is terminal.\n".to_string()
    } else {
        String::new()
    };
    let profile_rows = if matches!(profile, AgentProfile::Coder) {
        "Use **3–8** milestones in **`global_milestones`** when initialized, including **Recon**, **Implement**, and **Unit tests**.
Each milestone: `plan`, `done_when`; optional `remark` when marking `done`.
"
    } else if matches!(profile, AgentProfile::Computer) {
        COMPUTER_LOOP_INIT_ROWS
    } else {
        "Use **3–8** concise milestones in **`global_milestones`** for normal multi-step work.
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
    profile: &AgentProfile,
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
    let mut hint = sub_agent_hint_block(profile).to_string();
    if matches!(profile, AgentProfile::Computer) {
        hint.push_str("\n");
        hint.push_str(COMPUTER_LOOP_INIT_ROWS);
    }
    Some(hint)
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

fn last_task_board_hint_index(messages: &[ChatMessage]) -> Option<usize> {
    messages
        .iter()
        .rposition(|m| m.content.contains(TASK_BOARD_HINT_TAG))
}

fn is_explore_handoff_content(content: &str) -> bool {
    content.contains("## Summary") && content.contains("## Key files")
        || content.contains("## Impact map")
}

fn explore_handoff_since(messages: &[ChatMessage], after_hint: Option<usize>) -> bool {
    let start = after_hint.map(|i| i + 1).unwrap_or(0);
    messages
        .iter()
        .skip(start)
        .any(|m| matches!(m.role, Role::Tool) && is_explore_handoff_content(&m.content))
}

fn should_inject_coder_init_hint(messages: &[ChatMessage]) -> bool {
    let prior_hint = last_task_board_hint_index(messages);
    if prior_hint.is_none() {
        return true;
    }
    explore_handoff_since(messages, prior_hint)
}

/// Whether to append `[TASK_BOARD_HINT]` on this turn (deduped for Coder).
pub fn should_inject_main_agent_task_board_init_hint(
    profile: &AgentProfile,
    messages: &[ChatMessage],
) -> bool {
    match profile {
        AgentProfile::Coder => should_inject_coder_init_hint(messages),
        AgentProfile::Computer => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentProfile;
    use crate::models::{ChatMessage, Role};

    fn msg(role: Role, content: &str) -> ChatMessage {
        ChatMessage {
            id: "m".into(),
            role,
            content: content.into(),
            status: "done".into(),
            created_at: 0,
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
        }
    }

    #[test]
    fn main_agent_hint_for_coder_when_board_empty() {
        let store = TaskBoardStore::new();
        let hint = main_agent_task_board_init_hint(&store, "conv-1", &AgentProfile::Coder);
        let text = hint.expect("coder hint");
        assert!(text.contains("TASK_BOARD_HINT"));
        assert!(text.contains("task_board_init"));
    }

    #[test]
    fn main_agent_computer_hint_includes_loop_shape() {
        let store = TaskBoardStore::new();
        let hint = main_agent_task_board_init_hint(&store, "conv-1", &AgentProfile::Computer);
        let text = hint.expect("computer hint");
        assert!(text.contains("dynamic_quota"));
        assert!(text.contains("wi_*"));
    }

    #[test]
    fn sub_agent_computer_hint_includes_loop_shape() {
        let store = TaskBoardStore::new();
        let hint = sub_agent_task_board_init_hint(
            &store,
            "conv-child",
            &["task_board_init".to_string()],
            &AgentProfile::Computer,
        );
        let text = hint.expect("sub computer hint");
        assert!(text.contains("loop_item_plan"));
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

    #[test]
    fn coder_init_hint_first_turn_only_until_explore() {
        let empty: Vec<ChatMessage> = vec![];
        assert!(should_inject_main_agent_task_board_init_hint(
            &AgentProfile::Coder,
            &empty,
        ));
        let after_hint = vec![msg(
            Role::User,
            "[TASK_BOARD_HINT]\nYour task board is empty.",
        )];
        assert!(!should_inject_main_agent_task_board_init_hint(
            &AgentProfile::Coder,
            &after_hint,
        ));
        let after_explore = vec![
            msg(Role::User, "[TASK_BOARD_HINT]\nempty"),
            msg(
                Role::Tool,
                "## Summary\nx\n## Key files\n- a.rs\n## Evidence\n",
            ),
        ];
        assert!(should_inject_main_agent_task_board_init_hint(
            &AgentProfile::Coder,
            &after_explore,
        ));
    }

    #[test]
    fn computer_init_hint_when_board_empty() {
        let empty: Vec<ChatMessage> = vec![];
        assert!(should_inject_main_agent_task_board_init_hint(
            &AgentProfile::Computer,
            &empty,
        ));
    }
}
