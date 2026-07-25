//! Task board auto-init policy: complexity gate, hint text, and inject cadence (single source of truth).

use crate::agents::AgentProfile;
use crate::models::{ChatMessage, Role};

pub const TASK_BOARD_HINT_TAG: &str = "[TASK_BOARD_HINT";

const GATE_SIGNALS: &str = "\
**Default: skip `task_board_init`.** Init only for: user-requested plan/progress;
≥3 independently recoverable outcomes; batch N≥5; or ≥2 independent retry/branch paths.
**Never count:** explore → edit → test; one atomic outcome across many files; ≤5 linear GUI steps; parent/sub simple steps.";

const COMPUTER_LOOP: &str = "\
**Loop vs linear:** linear = 3–8 milestones. **Loop** only if enumerated N≥5 OR goal asks 汇总/逐条/批量/每个.
Enumerated → one `wi_*` per target; dynamic → `dynamic_quota` (host seeds wi_*). Shared SOP → `g_plan.plan`.
Patch terminal `wi_*`; host auto-advances; then `g_deliver`.";

const CODER_ROWS: &str = "\
Coder: file count and cross-module scope do not themselves justify a board. \
Skip init for one atomic change, even after explore. \
If the gate justifies a board, use task-specific units; never use a Recon→Implement→Verify ladder.";

const COMPUTER_VERIFY: &str = "Host verify runs after desktop tools; patch terminal `wi_*` rows.";

/// Compact `[TASK_BOARD_HINT]` for main or sub agent when the local board is empty.
pub fn build_init_hint(profile: &AgentProfile, is_sub_agent: bool) -> Option<String> {
    if !matches!(profile, AgentProfile::Computer | AgentProfile::Coder) {
        return None;
    }
    let sub = if is_sub_agent {
        "Sub-agent: same gate; read **[TASK_BOARD_PARENT]**; do not patch parent.\n"
    } else {
        ""
    };
    let profile_block = match profile {
        AgentProfile::Coder => format!("{CODER_ROWS}\n"),
        AgentProfile::Computer => format!("{COMPUTER_LOOP}\n{COMPUTER_VERIFY}\n"),
        _ => String::new(),
    };
    Some(format!(
        "[TASK_BOARD_HINT]
Board empty.
{GATE_SIGNALS}
{sub}{profile_block}Patch each substantive step; finalize when all terminal."
    ))
}

pub fn last_task_board_hint_index(messages: &[ChatMessage]) -> Option<usize> {
    messages
        .iter()
        .rposition(|m| m.content.contains(TASK_BOARD_HINT_TAG))
}

fn is_abandon_tool_result(content: &str) -> bool {
    content.contains("task_board_abandon") || content.contains("\"method\":\"abandon\"")
}

fn board_abandoned_since(messages: &[ChatMessage], after_hint: Option<usize>) -> bool {
    let start = after_hint.map(|i| i + 1).unwrap_or(0);
    messages
        .iter()
        .skip(start)
        .any(|m| matches!(m.role, Role::Tool) && is_abandon_tool_result(&m.content))
}

/// Whether to inject `[TASK_BOARD_HINT]` on this turn (first turn or after abandon).
///
/// An explore handoff is evidence gathering, not a scope upgrade. Reinjecting the
/// hint after every handoff encourages a generic Explore → Implement → Verify board.
pub fn should_inject_init_hint(profile: &AgentProfile, messages: &[ChatMessage]) -> bool {
    if !matches!(profile, AgentProfile::Computer | AgentProfile::Coder) {
        return false;
    }
    let prior = last_task_board_hint_index(messages);
    prior.is_none() || board_abandoned_since(messages, prior)
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn first_turn_only_until_board_abandoned() {
        let empty: Vec<ChatMessage> = vec![];
        assert!(should_inject_init_hint(&AgentProfile::Coder, &empty));
        assert!(should_inject_init_hint(&AgentProfile::Computer, &empty));

        let after_hint = vec![msg(Role::User, "[TASK_BOARD_HINT]\nempty")];
        assert!(!should_inject_init_hint(&AgentProfile::Coder, &after_hint));
        assert!(!should_inject_init_hint(
            &AgentProfile::Computer,
            &after_hint
        ));

        let after_explore = vec![
            msg(Role::User, "[TASK_BOARD_HINT]\nempty"),
            msg(
                Role::Tool,
                "## Summary\nx\n## Key files\n- a.rs\n## Evidence\n",
            ),
        ];
        assert!(!should_inject_init_hint(
            &AgentProfile::Coder,
            &after_explore
        ));

        let after_abandon = vec![
            msg(Role::User, "[TASK_BOARD_HINT]\nempty"),
            msg(Role::Tool, r#"{"method":"abandon"}"#),
        ];
        assert!(should_inject_init_hint(
            &AgentProfile::Computer,
            &after_abandon,
        ));
    }

    #[test]
    fn hint_includes_quantified_gate_and_sub_agent_parent() {
        let main = build_init_hint(&AgentProfile::Computer, false).expect("computer hint");
        assert!(main.contains("N≥5"));
        assert!(main.contains("≥3 independently recoverable outcomes"));
        assert!(main.contains("≥2 independent retry/branch paths"));
        assert!(main.contains("wi_*"));

        let sub = build_init_hint(&AgentProfile::Computer, true).expect("sub hint");
        assert!(sub.contains("TASK_BOARD_PARENT"));
    }
}
