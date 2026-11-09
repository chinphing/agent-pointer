//! Record tool execution outcome to stream, history, and computer state.

use crate::models::{StreamEvent, ToolCall};

use super::super::emit::{emit, trace_id_opt};
use super::super::util::{
    append_assistant_tool_raw_output, desktop_tool_failure_note, truncate_str,
};
use super::types::ToolPassContext;

/// What the tool pass still owes the UI after one tool call.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct PostActionSettle {
    /// Default settle to apply **once** at the end of the pass (`0` = nothing to do).
    pub round_default_ms: u64,
    /// The model asked for an explicit `wait` and it was already slept inline. The round-level
    /// default must not stack on top of it — one settle per pass, never two.
    pub explicit_waited: bool,
}

pub(super) async fn record_tool_exec_outcome(
    ctx: &mut ToolPassContext<'_>,
    tc: &ToolCall,
    tool_id: &str,
    args_for_desktop_log: &serde_json::Value,
    exec: super::types::ToolExecResult,
    duration: u64,
    trace_id: Option<&str>,
) -> PostActionSettle {
    // Post-action settle is a **round-level** concern: the next screenshot is captured at the
    // start of the next model round, never between two tools of this one. So the *default* is
    // returned to the caller and slept once at the end of the pass, while an explicit
    // `tool_args.wait` is a per-step gap and is slept right here.
    let mut settle = PostActionSettle::default();
    let persist = &ctx.persist;
    let scoped_message_id = ctx.sub.as_ref().map(|s| s.scoped_message_id.as_str());
    let conversation_id = ctx.session.conversation_id;
    let message_id = ctx.message_id.as_str();
    let stream = ctx.session.stream;
    let state = ctx.session.state;
    let run_id = ctx
        .lead
        .as_ref()
        .map(|l| l.run_id)
        .or_else(|| ctx.sub.as_ref().map(|s| s.instance_scope.run_id.as_str()))
        .unwrap_or_default();

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
                match crate::agents::computer::explicit_wait_delay_ms_from_tool_args(
                    args_for_desktop_log,
                ) {
                    // The model asked for a gap **after this step**: it must happen here, before
                    // the next tool of the same round — deferring it to the end would be wrong.
                    Some(wait_ms) => {
                        log::info!(
                            "desktop post_action explicit wait {}ms after tool (tool={})",
                            wait_ms,
                            tool_id
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                        settle.explicit_waited = true;
                    }
                    // No `wait` → the plain default, folded into the single end-of-pass settle.
                    None => {
                        settle.round_default_ms =
                            crate::agents::computer::POST_DESKTOP_ACTION_DELAY_MS;
                    }
                }
            }
            let preview = if tool_id == "list_apps" {
                truncate_str(&out, 12_000)
            } else {
                // file_edit / file_write results are short summaries (no diff_lines).
                truncate_str(&out, 800)
            };
            let display =
                state
                    .tools
                    .format_display_for(&tc.name, args_for_desktop_log, state.ui_locale());
            let (display_label, display_summary) =
                super::super::util::tool_display_stream_fields(&display);
            let stay_running = ok && crate::tools::job::host_tool_stays_running(tool_id, &out);
            let status = if stay_running {
                log::info!(
                    "tool outcome: host handle still running tool={tool_id} conversation_id={conversation_id} tool_call_id={}",
                    tc.id
                );
                "running"
            } else if ok {
                "success"
            } else {
                "failed"
            };
            state
                .hooks
                .run_post_tool_call(&crate::dispatcher::PostToolCallContext {
                    run_id,
                    conversation_id,
                    message_id,
                    tool_call_id: &tc.id,
                    tool_name: tool_id,
                    status,
                    result: Some(&out),
                    error: err_note.as_deref(),
                    state,
                })
                .await;
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
            let mut out = out;
            let mut monitor_note: Option<String> = None;
            super::super::computer_monitor_follow::maybe_auto_switch_capture_monitor_after_tool(
                ctx.session.stream,
                ctx.session.state,
                &settings,
                conversation_id,
                tool_id,
                ok,
                args_for_desktop_log,
                &mut monitor_note,
            );
            if let Some(note) = monitor_note {
                out = format!("{out}\n\n{note}");
            }
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
            state
                .hooks
                .run_post_tool_call(&crate::dispatcher::PostToolCallContext {
                    run_id,
                    conversation_id,
                    message_id,
                    tool_call_id: &tc.id,
                    tool_name: tool_id,
                    status: "failed",
                    result: None,
                    error: Some(err.as_str()),
                    state,
                })
                .await;
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
    settle
}
