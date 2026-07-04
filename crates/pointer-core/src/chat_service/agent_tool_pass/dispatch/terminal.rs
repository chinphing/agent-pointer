//! Terminal tool streaming execution.

use crate::models::{StreamEvent, ToolCall};
use crate::stream_broadcast::publish_stream;
use crate::tools::file::ConversationWorkspaceGuard;
use crate::tools::terminal::{
    run_terminal_command_streaming, terminal_stream_tool_status, TerminalInputHooks,
    TerminalInputResolution, TerminalNeedsInputPrompt,
};
use crate::tools::terminal::InputClass;
use anyhow::anyhow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use super::super::super::app_state::AppState;
use super::super::super::emit::trace_id_opt;
use super::super::super::StreamTx;
use super::super::types::ToolExecResult;

fn resolve_terminal_session_workspace(conversation_id: &str, from_settings: String) -> String {
    let trimmed = from_settings.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    if let Ok(store) = crate::conversation_store::global_store() {
        if let Ok(ws) = store.workspace_root(conversation_id) {
            if !ws.trim().is_empty() {
                log::info!(
                    "terminal: workspace from conversation store conversation_id={conversation_id}: {ws}"
                );
                return ws.trim().to_string();
            }
        }
    }
    let session_user_id = crate::conversation_store::global_store()
        .ok()
        .and_then(|store| store.session_user_id(conversation_id).ok())
        .unwrap_or_default();
    match crate::session_sandbox::SessionSandbox::ensure_default(
        conversation_id,
        session_user_id.as_str(),
    ) {
        Ok(path) => {
            let ws = path.display().to_string();
            log::info!(
                "terminal: workspace from session sandbox conversation_id={conversation_id}: {ws}"
            );
            ws
        }
        Err(e) => {
            log::warn!(
                "terminal: session sandbox ensure failed conversation_id={conversation_id}: {e:#}"
            );
            String::new()
        }
    }
}

fn input_class_wire(class: InputClass) -> String {
    match class {
        InputClass::Normal => "normal".to_string(),
        InputClass::Secret => "secret".to_string(),
    }
}

fn run_terminal_input_bridge(
    state: &AppState,
    stream: &StreamTx,
    msg_id_for_input: &str,
    tc_id_for_input: &str,
    trace_id_for_input: &Option<String>,
    scoped_for_input: &Option<String>,
    prompt_rx: std::sync::mpsc::Receiver<TerminalNeedsInputPrompt>,
    res_tx: std::sync::mpsc::Sender<TerminalInputResolution>,
) {
    while let Ok(prompt) = prompt_rx.recv() {
        let (tx, rx) = std::sync::mpsc::channel();
        state.register_terminal_input_wait(prompt.request_id.clone(), tx);
        let input_class = input_class_wire(prompt.input_class);
        publish_stream(
            stream,
            StreamEvent::TerminalNeedsInput {
                message_id: msg_id_for_input.to_string(),
                tool_call_id: tc_id_for_input.to_string(),
                request_id: prompt.request_id.clone(),
                command: prompt.command.clone(),
                output_context: prompt.output_context.clone(),
                input_hint: prompt.input_hint.clone(),
                input_class,
                trace_id: trace_id_for_input.clone(),
                scoped_message_id: scoped_for_input.clone(),
            },
        );
        log::info!(
            "terminal: TerminalNeedsInput published request_id={} tool_call_id={}",
            prompt.request_id,
            tc_id_for_input
        );
        let res = match rx.recv_timeout(Duration::from_millis(prompt.wait_for_input_ms)) {
            Ok(res) => res,
            Err(_) => {
                state
                    .terminal_input_pending
                    .lock()
                    .remove(&prompt.request_id);
                TerminalInputResolution::Dismiss
            }
        };
        if res_tx.send(res).is_err() {
            break;
        }
    }
}

pub(super) async fn run_terminal_tool(
    stream: &StreamTx,
    state: &AppState,
    conversation_id: &str,
    message_id: &str,
    tc: &ToolCall,
    args_value: serde_json::Value,
    cancel: &CancellationToken,
    trace_id: Option<String>,
    scoped_message_id: Option<String>,
    session_workspace: String,
) -> ToolExecResult {
    let session_workspace = resolve_terminal_session_workspace(conversation_id, session_workspace);
    if session_workspace.trim().is_empty() {
        log::warn!(
            "terminal: no session workspace; cwd may fall back to process directory conversation_id={conversation_id}"
        );
    }
    let cancel_terminal = cancel.clone();
    let abort_flag = Arc::new(AtomicBool::new(false));
    state.register_terminal_abort_flag(conversation_id, &tc.id, abort_flag.clone());
    let cleanup_conv = conversation_id.to_string();
    let cleanup_tc = tc.id.clone();
    let msg_id_for_stream = message_id.to_string();
    let tc_id_for_stream = tc.id.clone();
    let stream_for_terminal = stream.clone();
    let trace_id_for_terminal = trace_id_opt(trace_id.as_deref());
    let scoped_message_id_for_terminal = trace_id_opt(scoped_message_id.as_deref());

    let state_ptr = state as *const AppState as usize;
    let stream_ptr = stream as *const StreamTx as usize;
    let msg_id_for_input = message_id.to_string();
    let tc_id_for_input = tc.id.clone();
    let trace_id_for_input = trace_id_for_terminal.clone();
    let scoped_for_input = scoped_message_id_for_terminal.clone();

    let session_user_id = state
        .session_index
        .session_user_id(conversation_id)
        .unwrap_or_default();
    let session_user_id_for_blocking = session_user_id.clone();
    let session_work_dir_for_blocking = session_workspace.clone();
    let join = tokio::task::spawn_blocking(move || {
        let state = unsafe { &*(state_ptr as *const AppState) };
        let stream = unsafe { &*(stream_ptr as *const StreamTx) };

        let (prompt_tx, prompt_rx) = std::sync::mpsc::channel::<TerminalNeedsInputPrompt>();
        let (res_tx, res_rx) = std::sync::mpsc::channel::<TerminalInputResolution>();

        std::thread::scope(|scope| {
            scope.spawn(|| {
                run_terminal_input_bridge(
                    state,
                    stream,
                    &msg_id_for_input,
                    &tc_id_for_input,
                    &trace_id_for_input,
                    &scoped_for_input,
                    prompt_rx,
                    res_tx,
                );
            });

            let _workspace_guard = ConversationWorkspaceGuard::enter(session_workspace.clone());
            let _work_dir_guard = crate::session_work_dir_env::SessionWorkDirGuard::enter(
                session_work_dir_for_blocking.clone(),
            );
            let _session_user_guard = crate::session_user_env::SessionUserIdGuard::enter(
                session_user_id_for_blocking.clone(),
            );
            run_terminal_command_streaming(
                args_value,
                session_workspace,
                move |output| {
                    publish_stream(
                        &stream_for_terminal,
                        StreamEvent::TerminalOutputDelta {
                            message_id: msg_id_for_stream.clone(),
                            tool_call_id: tc_id_for_stream.clone(),
                            output: output.to_string(),
                            trace_id: trace_id_for_terminal.clone(),
                            scoped_message_id: scoped_message_id_for_terminal.clone(),
                        },
                    );
                },
                Some(cancel_terminal),
                Some(abort_flag),
                Some(TerminalInputHooks {
                    prompt_tx,
                    resolution_rx: res_rx,
                }),
            )
        })
        .map(|r| {
            let (ok, err_note) = terminal_stream_tool_status(&r);
            let body = serde_json::json!({
                "exitCode": r.exit_code,
                "success": r.success,
                "timedOut": r.timed_out,
                "cancelled": r.cancelled,
                "runAborted": r.run_aborted,
                "elevationDenied": r.elevation_denied,
                "needsInputLikely": r.needs_input_likely,
                "inputHint": r.input_hint,
                "inputClass": r.input_class,
                "agentRetryForbidden": r.agent_retry_forbidden,
                "userInputProvided": r.user_input_provided,
                "inputDismissed": r.input_dismissed,
                "waitedForInputMs": r.waited_for_input_ms,
                "durationMs": r.duration_ms,
                "stdout": r.stdout,
                "stderr": r.stderr,
                "stdoutTruncated": r.stdout_truncated,
                "stderrTruncated": r.stderr_truncated,
            })
            .to_string();
            (body, ok, err_note)
        })
    })
    .await;
    state.clear_terminal_abort_flag(&cleanup_conv, &cleanup_tc);
    join.map_err(|e| anyhow!("终端执行线程异常: {e}"))?
}
