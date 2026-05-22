//! Session evidence hints for soft task_board validation (host-injected, not model-authored).

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

#[cfg(test)]
mod tests {
    use super::*;
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
            agent_name: None,
            agent_trace: None,
            image_slot_labels: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
        }
    }

    #[test]
    fn detects_terminal_not_task_board() {
        let hist = vec![assistant_with_tools(&["task_board:patch", "terminal"])];
        assert!(history_has_recent_action_tools(&hist));
    }

    #[test]
    fn false_when_only_task_board() {
        let hist = vec![assistant_with_tools(&["task_board:patch"])];
        assert!(!history_has_recent_action_tools(&hist));
    }
}
