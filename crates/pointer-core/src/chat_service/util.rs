use crate::models::{ChatMessage, Role};
use crate::tools::ToolDisplay;

pub(crate) fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

pub(crate) fn truncate_str(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

pub(crate) fn tool_display_stream_fields(display: &ToolDisplay) -> (Option<String>, Option<String>) {
    let summary = if display.summary.is_empty() {
        None
    } else {
        Some(display.summary.clone())
    };
    (Some(display.label.clone()), summary)
}

pub(crate) fn patch_assistant_tool_call_display(
    history: &mut [ChatMessage],
    message_id: &str,
    tool_call_id: &str,
    display: &ToolDisplay,
) {
    let Some(msg) = history.iter_mut().find(|m| m.id == message_id) else {
        return;
    };
    let Some(tcs) = msg.tool_calls.as_mut() else {
        return;
    };
    let Some(tc) = tcs.iter_mut().find(|t| t.id == tool_call_id) else {
        return;
    };
    tc.display_label = Some(display.label.clone());
    tc.display_summary = if display.summary.is_empty() {
        None
    } else {
        Some(display.summary.clone())
    };
}

pub(crate) fn tool_result_msg(tool_call_id: &str, content: &str) -> ChatMessage {
    ChatMessage {
        id: new_id("tool"),
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
            }
}

pub(crate) fn push_tool_result(
    history: &mut Vec<ChatMessage>,
    conversation_id: &str,
    tool_call_id: &str,
    content: &str,
) {
    let msg = tool_result_msg(tool_call_id, content);
    history.push(msg.clone());
    super::conversation_persist::upsert_message(conversation_id, &msg);
}

pub(crate) fn append_assistant_tool_raw_output(
    history: &mut [ChatMessage],
    message_id: &str,
    tool_name: &str,
    tool_call_id: &str,
    tool_args: &serde_json::Value,
    raw_output: &str,
) {
    let Some(msg) = history.iter_mut().find(|m| m.id == message_id) else {
        return;
    };
    if !matches!(msg.role, Role::Assistant) {
        return;
    }
    let output = raw_output.trim();
    if output.is_empty() {
        return;
    }
    let args_text = compact_tool_log_text(tool_args.to_string().trim(), 1200);
    let output_text = compact_tool_log_text(output, 12000);
    let block = format!(
        "[tool:{} id:{}]\n[args]\n{}\n[output]\n{}",
        tool_name, tool_call_id, args_text, output_text
    );
    let buf = msg.tool_raw_output.get_or_insert_with(String::new);
    if !buf.is_empty() {
        buf.push_str("\n\n");
    }
    buf.push_str(&block);
}

fn compact_tool_log_text(s: &str, max_chars: usize) -> String {
    if s.is_empty() {
        return "(empty)".to_string();
    }
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let head: String = s.chars().take(max_chars).collect();
    format!("{head}…(+{} chars)", count - max_chars)
}

/// When the tool run did not succeed, short text for `[Recent desktop tool calls]` (`FAILED: …`).
pub(crate) fn desktop_tool_failure_note(
    ok: bool,
    err_note: &Option<String>,
    tool_output: &str,
) -> Option<String> {
    if ok {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    if let Some(e) = err_note.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        parts.push(e.to_string());
    }
    let out = truncate_str(tool_output, 200);
    if !out.trim().is_empty() {
        parts.push(out);
    }
    if parts.is_empty() {
        Some("failed".into())
    } else {
        Some(parts.join(" | "))
    }
}
