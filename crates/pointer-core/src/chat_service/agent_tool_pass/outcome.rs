//! Record tool execution outcome to stream, history, and computer state.

use crate::models::{StreamEvent, ToolCall};
use std::time::Duration;

use super::types::ToolPassContext;
use super::super::emit::{emit, trace_id_opt};
use super::super::util::{append_assistant_tool_raw_output, desktop_tool_failure_note, truncate_str};

pub(super) async fn record_tool_exec_outcome(
    ctx: &mut ToolPassContext<'_>,
    tc: &ToolCall,
    tool_id: &str,
    args_for_desktop_log: &serde_json::Value,
    exec: super::types::ToolExecResult,
    duration: u64,
    trace_id: Option<&str>,
) {
    let persist_transcript = ctx.persist_transcript();
    let conversation_id = ctx.session.conversation_id;
    let message_id = ctx.message_id.as_str();
    let stream = ctx.session.stream;
    let state = ctx.session.state;

    match exec {
        Ok((out, ok, err_note)) => {
            let failed_note = desktop_tool_failure_note(ok, &err_note, &out);
            state.computer_state.record_desktop_tool_if_applicable(
                conversation_id,
                tool_id,
                args_for_desktop_log,
                failed_note.as_deref(),
            );
            if ok && crate::agents::computer::is_desktop_post_delay_tool(tool_id) {
                let delay_ms =
                    crate::agents::computer::post_desktop_action_delay_ms_from_tool_args(
                        args_for_desktop_log,
                    );
                log::info!(
                    "desktop post_action sleep {}ms before next capture (tool={})",
                    delay_ms,
                    tool_id
                );
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            let preview = truncate_str(&out, 800);
            emit(
                stream,
                StreamEvent::ToolCallStatus {
                    message_id: message_id.to_string(),
                    tool_call_id: tc.id.clone(),
                    status: if ok { "success".into() } else { "failed".into() },
                    result: Some(preview),
                    error: err_note,
                    duration_ms: Some(duration),
                    display_label: None,
                    display_summary: None,
                    trace_id: trace_id_opt(trace_id),
                },
            );
            append_assistant_tool_raw_output(
                ctx.transcript.history,
                message_id,
                &tc.name,
                &tc.id,
                args_for_desktop_log,
                &out,
            );
            super::super::util::push_tool_result(
                ctx.transcript.history,
                conversation_id,
                message_id,
                &tc.id,
                &out,
                persist_transcript,
            );
        }
        Err(e) => {
            let err = e.to_string();
            let err_snip = truncate_str(&err, 400);
            state.computer_state.record_desktop_tool_if_applicable(
                conversation_id,
                tool_id,
                args_for_desktop_log,
                Some(err_snip.as_str()),
            );
            emit(
                stream,
                StreamEvent::ToolCallStatus {
                    message_id: message_id.to_string(),
                    tool_call_id: tc.id.clone(),
                    status: "failed".into(),
                    result: None,
                    error: Some(err.clone()),
                    duration_ms: Some(duration),
                    display_label: None,
                    display_summary: None,
                    trace_id: trace_id_opt(trace_id),
                },
            );
            let error_out = format!("ERROR: {err}");
            append_assistant_tool_raw_output(
                ctx.transcript.history,
                message_id,
                &tc.name,
                &tc.id,
                args_for_desktop_log,
                &error_out,
            );
            super::super::util::push_tool_result(
                ctx.transcript.history,
                conversation_id,
                message_id,
                &tc.id,
                &error_out,
                persist_transcript,
            );
        }
    }
}
