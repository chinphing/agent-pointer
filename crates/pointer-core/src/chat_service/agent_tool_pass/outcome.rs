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
    let persist = &ctx.persist;
    let scoped_message_id = ctx
        .sub
        .as_ref()
        .map(|s| s.scoped_message_id.as_str());
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
            let preview = if tool_id == "list_apps" {
                truncate_str(&out, 12_000)
            } else {
                truncate_str(&out, 800)
            };
            let display = state.tools.format_display(&tc.name, args_for_desktop_log);
            let (display_label, display_summary) =
                super::super::util::tool_display_stream_fields(&display);
            let status = if ok { "success" } else { "failed" };
            super::super::util::patch_assistant_tool_call_outcome(
                ctx.transcript.history,
                message_id,
                &tc.id,
                status,
                Some(preview.as_str()),
                err_note.as_deref(),
                Some(duration),
                display_label.as_deref(),
                display_summary.as_deref(),
            );
            emit(
                stream,
                StreamEvent::ToolCallStatus {
                    message_id: message_id.to_string(),
                    tool_call_id: tc.id.clone(),
                    status: status.into(),
                    result: Some(preview),
                    error: err_note,
                    duration_ms: Some(duration),
                    display_label,
                    display_summary,
                    trace_id: trace_id_opt(trace_id),
                    scoped_message_id: trace_id_opt(scoped_message_id),
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
            if let super::super::context::TranscriptPersist::SubLinked(linkage) = persist {
                super::super::sub_message::persist_scoped_assistant_snapshot(
                    conversation_id,
                    linkage,
                    ctx.transcript.history,
                    message_id,
                );
            }
            let settings = ctx.session.state.effective_settings();
            super::super::computer_monitor_follow::maybe_auto_switch_capture_monitor_after_tool(
                ctx.session.stream,
                ctx.session.state,
                &settings,
                conversation_id,
                tool_id,
                ok,
                args_for_desktop_log,
            );
            super::super::util::push_tool_result(
                ctx.transcript.history,
                conversation_id,
                message_id,
                &tc.id,
                &out,
                persist,
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
                    scoped_message_id: trace_id_opt(scoped_message_id),
                },
            );
            super::super::util::patch_assistant_tool_call_outcome(
                ctx.transcript.history,
                message_id,
                &tc.id,
                "failed",
                None,
                Some(err.as_str()),
                Some(duration),
                None,
                None,
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
            if let super::super::context::TranscriptPersist::SubLinked(linkage) = persist {
                super::super::sub_message::persist_scoped_assistant_snapshot(
                    conversation_id,
                    linkage,
                    ctx.transcript.history,
                    message_id,
                );
            }
            super::super::util::push_tool_result(
                ctx.transcript.history,
                conversation_id,
                message_id,
                &tc.id,
                &error_out,
                persist,
            );
        }
    }
}
