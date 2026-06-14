//! Tool approval gate (manual mode / elevated terminal).

use crate::models::{ChatMessage, StreamEvent, ToolCall};
use anyhow::Result;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::super::app_state::AppState;
use super::super::emit::{emit, trace_id_opt};
use super::super::StreamTx;

pub(super) async fn run_approval_gate(
    stream: &StreamTx,
    state: &AppState,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    tool_approval_mode: &str,
    message_id: &str,
    tc: &ToolCall,
    tool_id: &str,
    args_value: &serde_json::Value,
    cancel: &CancellationToken,
    trace_id: Option<&str>,
    persist_transcript: bool,
) -> Result<bool> {
    let elevated_terminal =
        tool_id == "terminal" && crate::tools::terminal::terminal_requests_elevation(args_value);
    let requires_approval = elevated_terminal
        || (tool_approval_mode == "manual"
            && state
                .tools
                .tool_invocation_needs_approval(tool_id, args_value));
    if !requires_approval {
        return Ok(true);
    }

    emit(
        stream,
        StreamEvent::ToolCallStatus {
            message_id: message_id.to_string(),
            tool_call_id: tc.id.clone(),
            status: "pending_approval".into(),
            result: None,
            error: None,
            duration_ms: None,
            display_label: None,
            display_summary: None,
            trace_id: trace_id_opt(trace_id),
        },
    );
    let (atx, arx) = oneshot::channel::<bool>();
    state.approvals.lock().insert(tc.id.clone(), atx);
    let approved = tokio::select! {
        _ = cancel.cancelled() => {
            state.approvals.lock().remove(&tc.id);
            false
        }
        v = arx => v.unwrap_or(false),
    };
    if approved {
        return Ok(true);
    }
    let err = "用户已拒绝该工具调用".to_string();
    emit(
        stream,
        StreamEvent::ToolCallStatus {
            message_id: message_id.to_string(),
            tool_call_id: tc.id.clone(),
            status: "rejected".into(),
            result: None,
            error: Some(err.clone()),
            duration_ms: None,
            display_label: None,
            display_summary: None,
            trace_id: trace_id_opt(trace_id),
        },
    );
    super::super::util::push_tool_result(
        history,
        conversation_id,
        message_id,
        &tc.id,
        &err,
        persist_transcript,
    );
    Ok(false)
}
