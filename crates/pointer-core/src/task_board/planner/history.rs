//! Ephemeral planner message history (no execution inject / tool noise).

use crate::message_context::{filter_context_messages, is_synthetic_user_content};
use crate::models::{ChatMessage, Role};
use crate::task_board::history_trim::is_injected_or_synthetic_user_content;

const TASK_BOARD_TAG: &str = "[TASK_BOARD";
const TASK_BOARD_HINT_TAG: &str = "[TASK_BOARD_HINT";
const MAX_ASSISTANT_CHARS: usize = 12_000;

fn is_planner_excluded_user_content(content: &str) -> bool {
    let t = content.trim_start();
    is_injected_or_synthetic_user_content(content)
        || t.starts_with(TASK_BOARD_TAG)
        || t.starts_with(TASK_BOARD_HINT_TAG)
}

fn is_sub_agent_scoped(m: &ChatMessage) -> bool {
    m.spawn_depth.unwrap_or(0) > 0
}

fn sanitize_assistant(mut m: ChatMessage) -> ChatMessage {
    m.tool_calls = None;
    m.raw_content = None;
    m.tool_raw_output = None;
    m.images_base64 = None;
    m.image_slot_labels = None;
    m.computer_round_screen_rel_path = None;
    if m.content.chars().count() > MAX_ASSISTANT_CHARS {
        m.content = crate::text_util::truncate_chars(&m.content, MAX_ASSISTANT_CHARS);
    }
    m
}

/// Build planner-visible history from the lead transcript buffer.
pub fn build_planner_history(raw: &[ChatMessage]) -> Vec<ChatMessage> {
    let filtered = filter_context_messages(raw);
    let mut out = Vec::new();
    for m in filtered {
        if is_sub_agent_scoped(&m) {
            continue;
        }
        match m.role {
            Role::User => {
                if is_planner_excluded_user_content(&m.content) {
                    continue;
                }
                if is_synthetic_user_content(&m.content) {
                    // Compression / tool-limit summaries are kept for planning context.
                    out.push(m);
                    continue;
                }
                out.push(m);
            }
            Role::Assistant => {
                if m.content.trim().is_empty() && m.tool_calls.is_none() {
                    continue;
                }
                out.push(sanitize_assistant(m));
            }
            Role::System | Role::Tool => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Role;

    fn user(content: &str) -> ChatMessage {
        ChatMessage {
            id: "u1".into(),
            role: Role::User,
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

    fn assistant(content: &str, with_tools: bool) -> ChatMessage {
        ChatMessage {
            id: "a1".into(),
            role: Role::Assistant,
            content: content.into(),
            status: "done".into(),
            created_at: 0,
            tool_calls: with_tools.then(|| {
                vec![crate::models::ToolCall {
                    id: "tc1".into(),
                    name: "mouse_click".into(),
                    arguments: "{}".into(),
                    status: "done".into(),
                    result: None,
                    error: None,
                    duration_ms: None,
                    risk_level: None,
                    display_label: None,
                    display_summary: None,
                }]
            }),
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
    fn sanitize_assistant_truncates_by_char_not_byte() {
        let long = "描".repeat(MAX_ASSISTANT_CHARS + 1);
        let out = sanitize_assistant(assistant(&long, false));
        assert!(out.content.ends_with('…'));
        assert_eq!(out.content.chars().count(), MAX_ASSISTANT_CHARS + 1);
    }

    #[test]
    fn strips_task_board_inject_and_assistant_tool_calls() {
        let raw = vec![
            user("open ten apps"),
            user("[TASK_BOARD]\nstore_key: c1\n..."),
            assistant("clicking", true),
        ];
        let hist = build_planner_history(&raw);
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].content, "open ten apps");
        assert!(hist[1].tool_calls.is_none());
    }
}
