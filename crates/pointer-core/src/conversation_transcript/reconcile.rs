use crate::models::{ChatMessage, Role};

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn tool_message_id(tool_call_id: &str) -> String {
    let safe: String = tool_call_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("tool_{safe}")
}

pub fn tool_result_message(tool_call_id: &str, content: &str) -> ChatMessage {
    ChatMessage {
        id: tool_message_id(tool_call_id),
        role: Role::Tool,
        content: content.to_string(),
        status: "completed".into(),
        created_at: now_ms(),
        tool_calls: None,
        tool_call_id: Some(tool_call_id.to_string()),
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

fn assistant_has_tool_call(msg: &ChatMessage, tool_call_id: &str) -> bool {
    if !matches!(msg.role, Role::Assistant) {
        return false;
    }
    msg.tool_calls
        .as_ref()
        .is_some_and(|tcs| tcs.iter().any(|t| t.id == tool_call_id))
}

pub fn insert_after_tool_block(messages: &[ChatMessage], assistant_idx: usize) -> usize {
    let mut j = assistant_idx + 1;
    while j < messages.len() && matches!(messages[j].role, Role::Tool) {
        j += 1;
    }
    j
}

pub fn find_tool_insert_index(
    messages: &[ChatMessage],
    tool_call_id: &str,
    hint_message_id: &str,
) -> Option<usize> {
    if let Some(i) = messages.iter().position(|m| m.id == hint_message_id) {
        if assistant_has_tool_call(&messages[i], tool_call_id) {
            return Some(insert_after_tool_block(messages, i));
        }
    }
    for i in (0..messages.len()).rev() {
        if assistant_has_tool_call(&messages[i], tool_call_id) {
            return Some(insert_after_tool_block(messages, i));
        }
    }
    None
}

pub fn find_existing_tool_index(
    messages: &[ChatMessage],
    tool_call_id: &str,
    hint_message_id: &str,
) -> Option<usize> {
    let stable_id = tool_message_id(tool_call_id);
    if let Some(i) = messages.iter().position(|m| m.id == stable_id) {
        return Some(i);
    }
    if let Some(a_idx) = messages.iter().position(|m| m.id == hint_message_id) {
        if assistant_has_tool_call(&messages[a_idx], tool_call_id) {
            let start = a_idx + 1;
            let end = insert_after_tool_block(messages, a_idx);
            for i in start..end {
                if messages[i].tool_call_id.as_deref() == Some(tool_call_id) {
                    return Some(i);
                }
            }
        }
    }
    for i in (0..messages.len()).rev() {
        if !assistant_has_tool_call(&messages[i], tool_call_id) {
            continue;
        }
        let start = i + 1;
        let end = insert_after_tool_block(messages, i);
        for j in start..end {
            if messages[j].tool_call_id.as_deref() == Some(tool_call_id) {
                return Some(j);
            }
        }
        break;
    }
    None
}

/// Remove tool rows that are not paired with a preceding assistant `tool_calls` entry.
pub fn reconcile_tool_messages(messages: &mut Vec<ChatMessage>) -> bool {
    let mut changed = false;
    let mut i = 0usize;
    while i < messages.len() {
        if !matches!(messages[i].role, Role::Tool) {
            i += 1;
            continue;
        }
        let tool_call_id = messages[i]
            .tool_call_id
            .as_deref()
            .unwrap_or("")
            .to_string();
        let valid = if tool_call_id.is_empty() {
            false
        } else {
            messages[..i]
                .iter()
                .rev()
                .any(|m| assistant_has_tool_call(m, &tool_call_id))
        };
        if valid {
            i += 1;
        } else {
            log::warn!(
                "conversation_transcript: reconcile removing orphan tool message_id={} tool_call_id={tool_call_id}",
                messages[i].id
            );
            messages.remove(i);
            changed = true;
        }
    }
    changed
}

pub fn maybe_update_preview(preview: &mut String, msg: &ChatMessage) {
    if preview.is_empty() {
        *preview = crate::conversation_store::conversation_preview(&[msg.clone()]);
    }
}
