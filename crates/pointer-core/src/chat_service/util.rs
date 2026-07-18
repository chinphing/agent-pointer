use crate::models::{ChatMessage, Role};

use super::context::TranscriptPersist;
use super::sub_message::{self};
use crate::tools::ToolDisplay;

/// Strip wire-only image payloads from all messages after a chat round.
/// Each screenshot can be 500 KB–2 MB in base64; without this, history Vec
/// grows unbounded across turns in long-running sessions.
pub(crate) fn strip_images_from_history(history: &mut [ChatMessage]) {
    for m in history.iter_mut() {
        m.images_base64 = None;
        m.image_slot_labels = None;
    }
}

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
    crate::text_util::truncate_chars(s, n)
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
    let Some(tc) = find_assistant_tool_call_mut(history, message_id, tool_call_id) else {
        return;
    };
    tc.display_label = Some(display.label.clone());
    tc.display_summary = if display.summary.is_empty() {
        None
    } else {
        Some(display.summary.clone())
    };
}

pub(crate) fn patch_assistant_tool_call_outcome(
    history: &mut [ChatMessage],
    message_id: &str,
    tool_call_id: &str,
    status: &str,
    result: Option<&str>,
    error: Option<&str>,
    duration_ms: Option<u64>,
    display_label: Option<&str>,
    display_summary: Option<&str>,
) -> bool {
    let Some(tc) = find_assistant_tool_call_mut(history, message_id, tool_call_id) else {
        return false;
    };
    tc.status = status.to_string();
    tc.result = result.map(str::to_string);
    tc.error = error.map(str::to_string);
    tc.duration_ms = duration_ms;
    if let Some(label) = display_label {
        tc.display_label = Some(label.to_string());
    }
    if let Some(summary) = display_summary {
        tc.display_summary = Some(summary.to_string());
    }
    true
}

fn find_assistant_tool_call_mut<'a>(
    history: &'a mut [ChatMessage],
    message_id: &str,
    tool_call_id: &str,
) -> Option<&'a mut crate::models::ToolCall> {
    let msg = history.iter_mut().find(|m| m.id == message_id)?;
    let tcs = msg.tool_calls.as_mut()?;
    tcs.iter_mut().find(|t| t.id == tool_call_id)
}

pub(crate) fn push_tool_result(
    history: &mut Vec<ChatMessage>,
    conversation_id: &str,
    hint_message_id: &str,
    tool_call_id: &str,
    content: &str,
    persist: &TranscriptPersist,
) {
    match persist {
        TranscriptPersist::Main => {
            crate::conversation_transcript::record_tool_result(
                conversation_id,
                history,
                hint_message_id,
                tool_call_id,
                content,
            );
        }
        TranscriptPersist::SubLinked(linkage) => {
            sub_message::push_sub_tool_result(
                history,
                conversation_id,
                hint_message_id,
                tool_call_id,
                content,
                linkage,
            );
        }
    }
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
