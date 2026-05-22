use crate::models::{ChatMessage, Role};

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
        agent_id: None,
        agent_name: None,
        agent_trace: None,
        image_slot_labels: None,
        images_base64: None,
        computer_round_screen_rel_path: None,
    }
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
