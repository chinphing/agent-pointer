//! Session evidence hints for soft task_board validation (host-injected, not model-authored).

use crate::agents::computer::tool_names::is_action_verify_tool_name;
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

/// Whether recent chat history includes an `action_verify` sidecar with `action_result=pass`.
pub fn history_has_recent_verify_pass(history: &[ChatMessage]) -> bool {
    for msg in history.iter().rev().take(RECENT_MESSAGE_SCAN) {
        if !matches!(msg.role, Role::Assistant) {
            continue;
        }
        let Some(calls) = msg.tool_calls.as_ref() else {
            continue;
        };
        for tc in calls {
            if !is_action_verify_tool_name(tc.name.trim()) {
                continue;
            }
            let parsed = serde_json::from_str::<serde_json::Value>(&tc.arguments);
            let Ok(v) = parsed else {
                continue;
            };
            if v.get("action_result")
                .and_then(|x| x.as_str())
                .map(|s| s.eq_ignore_ascii_case("pass"))
                .unwrap_or(false)
            {
                return true;
            }
        }
    }
    false
}

/// Whether recent history includes any `action_verify` sidecar call.
pub fn history_has_recent_verify_report(history: &[ChatMessage]) -> bool {
    for msg in history.iter().rev().take(RECENT_MESSAGE_SCAN) {
        if !matches!(msg.role, Role::Assistant) {
            continue;
        }
        let Some(calls) = msg.tool_calls.as_ref() else {
            continue;
        };
        if calls.iter().any(|tc| is_action_verify_tool_name(tc.name.trim())) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::computer::tool_names::ACTION_VERIFY;
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
    fn detects_recent_verify_pass() {
        let mut msg = assistant_with_tools(&[ACTION_VERIFY]);
        if let Some(calls) = msg.tool_calls.as_mut() {
            calls[0].arguments = r#"{"action_result":"pass","repetition_count":0}"#.into();
        }
        assert!(history_has_recent_verify_pass(&[msg]));
    }

    #[test]
    fn verify_fail_does_not_count_as_pass() {
        let mut msg = assistant_with_tools(&[ACTION_VERIFY]);
        if let Some(calls) = msg.tool_calls.as_mut() {
            calls[0].arguments =
                r#"{"action_result":"fail","repetition_count":2,"failure_cause":"precision_miss"}"#.into();
        }
        assert!(!history_has_recent_verify_pass(&[msg]));
    }

    #[test]
    fn detects_recent_verify_report() {
        let msg = assistant_with_tools(&[ACTION_VERIFY]);
        assert!(history_has_recent_verify_report(&[msg]));
    }
}
