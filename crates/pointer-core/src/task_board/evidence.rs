//! Session evidence hints for soft task_board validation (host-injected, not model-authored).

use crate::chat_service::AppState;
use crate::models::{ChatMessage, Role};
use crate::task_board::checkpoint::is_task_board_tool_name;

const RECENT_MESSAGE_SCAN: usize = 64;

/// Whether recent chat history includes a non–`task_board` tool invocation (assistant `tool_calls`).
pub fn history_has_recent_action_tools(history: &[ChatMessage]) -> bool {
    for msg in history.iter().rev().take(RECENT_MESSAGE_SCAN) {
        if !matches!(msg.role, Role::Assistant) {
            continue;
        }
        let Some(calls) = msg.tool_calls.as_ref() else {
            continue;
        };
        if calls
            .iter()
            .any(|tc| !is_task_board_tool_name(tc.name.trim()))
        {
            return true;
        }
    }
    false
}

/// Whether tier runtime history includes a recent host verify pass.
pub fn history_has_recent_verify_pass(state: &AppState, conversation_id: &str) -> bool {
    state
        .computer_state
        .tier_history_has_verify_pass(conversation_id)
}

/// Whether tier runtime history includes any closed host verify report.
pub fn history_has_recent_verify_report(state: &AppState, conversation_id: &str) -> bool {
    state
        .computer_state
        .tier_history_has_verify_report(conversation_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::computer::ComputerState;
    use crate::models::{ChatMessage, Role, ToolCall};

    fn assistant_with_tools(names: &[&str]) -> ChatMessage {
        ChatMessage {
            id: "a1".into(),
            role: Role::Assistant,
            content: String::new(),
            status: "done".into(),
            created_at: 0,
            tool_calls: Some(
                names
                    .iter()
                    .enumerate()
                    .map(|(i, n)| ToolCall {
                        id: format!("tc{i}"),
                        name: (*n).to_string(),
                        arguments: "{}".into(),
                        status: "completed".into(),
                        result: None,
                        error: None,
                        duration_ms: None,
                        risk_level: None,
                        display_label: None,
                        display_summary: None,
                    })
                    .collect(),
            ),
            tool_call_id: None,
            tool_name: None,
            error_message: None,
            reasoning: None,
            thoughts: None,
            headline: None,
            raw_content: None,
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
            tool_raw_output: None,
        }
    }

    #[test]
    fn detects_terminal_not_task_board() {
        let hist = vec![assistant_with_tools(&["task_board_patch", "terminal"])];
        assert!(history_has_recent_action_tools(&hist));
    }

    #[test]
    fn false_when_only_task_board() {
        let hist = vec![assistant_with_tools(&["task_board_patch"])];
        assert!(!history_has_recent_action_tools(&hist));
    }

    #[test]
    fn tier_history_detects_verify_pass() {
        let state = ComputerState::with_annotate_url("http://127.0.0.1:9999");
        let conv = "evidence-pass";
        state.record_desktop_tool_if_applicable(
            conv,
            "mouse_click_index",
            &serde_json::json!({"goal": "g", "index": 1}),
            None,
        );
        state.apply_pipeline_verify_result(
            conv,
            &crate::agents::computer::pipeline::types::VerifyConclusion {
                action_result: crate::agents::computer::pipeline::types::ActionResult::Pass,
                failure_cause: None,
                step_summary: Some("ok".into()),
            },
        );
        assert!(state.tier_history_has_verify_pass(conv));
    }
}
