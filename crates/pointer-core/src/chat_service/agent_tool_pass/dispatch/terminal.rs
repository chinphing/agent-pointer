//! Terminal tool streaming execution.

use crate::chat_service::job_supervisor::{
    AwaitMode, JobKind, JobKindTerminal, JobStatus, JobSupervisor,
};
use crate::chat_service::run_subagent_delegation::{
    background_job_handle_json, emit_and_persist_host_tool_finish, emit_background_jobs,
};
use crate::models::{StreamEvent, ToolCall};
use crate::stream_broadcast::publish_stream;
use crate::tools::file::ConversationWorkspaceGuard;
use crate::tools::parallel::ParallelLimits;
use crate::tools::terminal::InputClass;
use crate::tools::terminal::{
    parse_block_until_ms, run_terminal_command_streaming, terminal_requests_elevation,
    terminal_stream_tool_status, terminal_streaming_result_json, TerminalInputHooks,
    TerminalInputResolution, TerminalNeedsInputPrompt, TerminalStreamingResult,
};
use anyhow::anyhow;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use super::super::super::app_state::{AppState, ToolExecutionScope};
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
    execution_scope: &ToolExecutionScope,
    trace_id_for_input: &Option<String>,
    scoped_for_input: &Option<String>,
    prompt_rx: std::sync::mpsc::Receiver<TerminalNeedsInputPrompt>,
    res_tx: std::sync::mpsc::Sender<TerminalInputResolution>,
) {
    while let Ok(prompt) = prompt_rx.recv() {
        let (tx, rx) = std::sync::mpsc::channel();
        state.register_terminal_input_wait(execution_scope.clone(), prompt.request_id.clone(), tx);
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
            "terminal: TerminalNeedsInput published request_id={} {}",
            prompt.request_id,
            execution_scope.log_fields()
        );
        let res = match rx.recv_timeout(Duration::from_millis(prompt.wait_for_input_ms)) {
            Ok(res) => res,
            Err(_) => {
                state.clear_terminal_input_wait(execution_scope, &prompt.request_id);
                log::info!(
                    "terminal: input wait timed out request_id={} {}",
                    prompt.request_id,
                    execution_scope.log_fields()
                );
                TerminalInputResolution::Dismiss
            }
        };
        if res_tx.send(res).is_err() {
            break;
        }
    }
}

fn terminal_job_title(args: &serde_json::Value) -> (String, Option<String>) {
    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let label = args
        .get("label")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    (command, label)
}

fn cancelled_terminal_json() -> String {
    terminal_streaming_result_json(&TerminalStreamingResult {
        exit_code: None,
        success: false,
        timed_out: false,
        cancelled: true,
        run_aborted: false,
        elevation_denied: false,
        needs_input_likely: false,
        input_hint: None,
        input_class: InputClass::Normal,
        agent_retry_forbidden: false,
        user_input_provided: false,
        input_dismissed: false,
        waited_for_input_ms: 0,
        duration_ms: 0,
        stdout: String::new(),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
    })
}

fn job_status_from_terminal(r: &TerminalStreamingResult, job_cancel: &CancellationToken) -> JobStatus {
    let (ok, _) = terminal_stream_tool_status(r);
    if r.cancelled || job_cancel.is_cancelled() {
        JobStatus::Cancelled
    } else if ok {
        JobStatus::Completed
    } else {
        JobStatus::Failed
    }
}

struct BackgroundTerminalSpawn {
    stream: StreamTx,
    state: Arc<AppState>,
    conversation_id: String,
    message_id: String,
    tool_call_id: String,
    args_value: serde_json::Value,
    session_workspace: String,
    session_user_id: String,
    execution_scope: ToolExecutionScope,
    cancel: CancellationToken,
    host_trace_id: Option<String>,
    host_scoped_message_id: Option<String>,
    abort_flag: Arc<AtomicBool>,
}

pub(super) async fn run_terminal_tool(
    stream: &StreamTx,
    state: &AppState,
    state_arc: Arc<AppState>,
    conversation_id: &str,
    message_id: &str,
    tc: &ToolCall,
    args_value: serde_json::Value,
    cancel: &CancellationToken,
    trace_id: Option<String>,
    scoped_message_id: Option<String>,
    session_workspace: String,
    execution_scope: ToolExecutionScope,
) -> ToolExecResult {
    let session_workspace = resolve_terminal_session_workspace(conversation_id, session_workspace);
    if session_workspace.trim().is_empty() {
        log::warn!(
            "terminal: no session workspace; cwd may fall back to process directory conversation_id={conversation_id}"
        );
    }

    let block_until = parse_block_until_ms(&args_value).map_err(|e| anyhow!(e))?;
    if block_until.is_some() && terminal_requests_elevation(&args_value) {
        log::warn!(
            "terminal: rejected elevated + blockUntilMs conversation_id={conversation_id} tool_call_id={}",
            tc.id
        );
        return Err(anyhow!(
            "background terminal cannot use elevated: true; omit blockUntilMs to run elevated in the foreground"
        ));
    }

    if let Some(block_until_ms) = block_until {
        return run_terminal_background(
            stream,
            state,
            state_arc,
            conversation_id,
            message_id,
            tc,
            args_value,
            cancel,
            trace_id,
            scoped_message_id,
            session_workspace,
            execution_scope,
            block_until_ms,
        )
        .await;
    }

    run_terminal_foreground(
        stream,
        state,
        conversation_id,
        message_id,
        tc,
        args_value,
        cancel,
        trace_id,
        scoped_message_id,
        session_workspace,
        execution_scope,
    )
    .await
}

async fn run_terminal_foreground(
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
    execution_scope: ToolExecutionScope,
) -> ToolExecResult {
    let cancel_terminal = cancel.clone();
    let abort_flag = Arc::new(AtomicBool::new(false));
    state.register_terminal_abort_flag(execution_scope.clone(), abort_flag.clone());
    let cleanup_scope = execution_scope.clone();
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
    let execution_scope_for_input = execution_scope.clone();

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
                    &execution_scope_for_input,
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
            let body = terminal_streaming_result_json(&r);
            (body, ok, err_note)
        })
    })
    .await;
    state.clear_terminal_abort_flag(&cleanup_scope);
    log::info!("terminal: completed {}", execution_scope.log_fields());
    join.map_err(|e| anyhow!("终端执行线程异常: {e}"))?
}

async fn run_terminal_background(
    stream: &StreamTx,
    state: &AppState,
    state_arc: Arc<AppState>,
    conversation_id: &str,
    message_id: &str,
    tc: &ToolCall,
    args_value: serde_json::Value,
    parent_cancel: &CancellationToken,
    trace_id: Option<String>,
    scoped_message_id: Option<String>,
    session_workspace: String,
    execution_scope: ToolExecutionScope,
    block_until_ms: u64,
) -> ToolExecResult {
    let job_cancel = CancellationToken::new();
    let (command, label) = terminal_job_title(&args_value);
    let kind = JobKind::Terminal(JobKindTerminal {
        tool_call_id: tc.id.clone(),
        message_id: message_id.to_string(),
        command,
        label,
    });
    let job_id = state
        .jobs
        .register(conversation_id, kind, job_cancel.clone());
    emit_background_jobs(
        stream,
        conversation_id,
        state.jobs.running_count_for_conversation(conversation_id),
    );
    log::info!(
        "terminal background spawn job_id={job_id} conversation_id={conversation_id} tool_call_id={} block_until_ms={block_until_ms}",
        tc.id
    );

    let abort_flag = Arc::new(AtomicBool::new(false));
    state.register_terminal_abort_flag(execution_scope.clone(), abort_flag.clone());
    let session_user_id = state
        .session_index
        .session_user_id(conversation_id)
        .unwrap_or_default();
    let spawn = BackgroundTerminalSpawn {
        stream: stream.clone(),
        state: state_arc,
        conversation_id: conversation_id.to_string(),
        message_id: message_id.to_string(),
        tool_call_id: tc.id.clone(),
        args_value,
        session_workspace,
        session_user_id,
        execution_scope,
        cancel: job_cancel,
        host_trace_id: trace_id,
        host_scoped_message_id: scoped_message_id,
        abort_flag,
    };
    tokio::spawn(run_background_terminal(job_id.clone(), spawn));

    let handle = background_job_handle_json(&job_id, JobStatus::Running, "terminal");
    if block_until_ms == 0 {
        log::info!(
            "terminal background detached immediately job_id={job_id} conversation_id={conversation_id}"
        );
        return Ok((handle, true, None));
    }

    let slot_cap = JobSupervisor::slot_cap_from(
        ParallelLimits::from_settings(&state.effective_settings()).max_parallel_sub_agents,
    );
    log::info!(
        "terminal background waiting job_id={job_id} conversation_id={conversation_id} block_until_ms={block_until_ms}"
    );
    let result = state
        .jobs
        .await_jobs(
            conversation_id,
            Some(vec![job_id.clone()]),
            AwaitMode::All,
            Some(Duration::from_millis(block_until_ms)),
            parent_cancel,
            slot_cap,
        )
        .await;

    if let Some(item) = result.jobs.into_iter().next() {
        if item.status != "queued" && item.status != "running" {
            let _ = state.jobs.claim_if_unclaimed(&job_id);
            let body = item.content.unwrap_or_else(|| {
                background_job_handle_json(&job_id, status_from_wire(&item.status), "terminal")
            });
            let ok = item.status == "completed";
            log::info!(
                "terminal background finished within wait job_id={job_id} status={} claimed=true",
                item.status
            );
            return Ok((body, ok, item.error));
        }
    }

    if let Some(item) = state.jobs.claim_if_unclaimed(&job_id) {
        let body = item.content.unwrap_or_else(|| {
            background_job_handle_json(&job_id, status_from_wire(item.status), "terminal")
        });
        let ok = item.status == "completed";
        log::info!(
            "terminal background claimed after wait race job_id={job_id} status={}",
            item.status
        );
        return Ok((body, ok, item.error));
    }

    log::info!(
        "terminal background still running after wait job_id={job_id} conversation_id={conversation_id}"
    );
    Ok((handle, true, None))
}

fn status_from_wire(status: &str) -> JobStatus {
    match status {
        "completed" => JobStatus::Completed,
        "failed" => JobStatus::Failed,
        "cancelled" => JobStatus::Cancelled,
        "queued" => JobStatus::Queued,
        _ => JobStatus::Running,
    }
}

async fn run_background_terminal(job_id: String, spawn: BackgroundTerminalSpawn) {
    let cap = JobSupervisor::slot_cap_from(
        ParallelLimits::from_settings(&spawn.state.effective_settings()).max_parallel_sub_agents,
    );
    if !spawn.state.jobs.acquire_slot(cap, &spawn.cancel).await {
        log::info!(
            "terminal background cancelled before slot job_id={job_id} conversation_id={}",
            spawn.conversation_id
        );
        let body = cancelled_terminal_json();
        spawn.state.jobs.finish(
            &job_id,
            JobStatus::Cancelled,
            Some(body.clone()),
            Some("cancelled".into()),
        );
        spawn
            .state
            .clear_terminal_abort_flag(&spawn.execution_scope);
        emit_and_persist_host_tool_finish(
            &spawn.stream,
            &spawn.conversation_id,
            &spawn.message_id,
            &spawn.tool_call_id,
            "failed",
            body,
            Some("cancelled"),
            Some(0),
            spawn.host_trace_id.as_deref(),
            spawn.host_scoped_message_id.as_deref(),
        );
        emit_background_jobs(
            &spawn.stream,
            &spawn.conversation_id,
            spawn
                .state
                .jobs
                .running_count_for_conversation(&spawn.conversation_id),
        );
        return;
    }
    spawn.state.jobs.mark_running(&job_id);
    log::info!(
        "terminal background running job_id={job_id} conversation_id={}",
        spawn.conversation_id
    );

    let cancel_terminal = spawn.cancel.clone();
    let abort_flag = spawn.abort_flag.clone();
    let session_workspace = spawn.session_workspace.clone();
    let session_user_id = spawn.session_user_id.clone();
    let args_value = spawn.args_value.clone();
    let stream_for_terminal = spawn.stream.clone();
    let msg_id_for_stream = spawn.message_id.clone();
    let tc_id_for_stream = spawn.tool_call_id.clone();
    let trace_id_for_terminal = trace_id_opt(spawn.host_trace_id.as_deref());
    let scoped_message_id_for_terminal = trace_id_opt(spawn.host_scoped_message_id.as_deref());
    let session_work_dir_for_blocking = session_workspace.clone();

    let join = tokio::task::spawn_blocking(move || {
        let _workspace_guard = ConversationWorkspaceGuard::enter(session_workspace.clone());
        let _work_dir_guard =
            crate::session_work_dir_env::SessionWorkDirGuard::enter(session_work_dir_for_blocking);
        let _session_user_guard =
            crate::session_user_env::SessionUserIdGuard::enter(session_user_id);
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
            None,
        )
    })
    .await;

    spawn
        .state
        .clear_terminal_abort_flag(&spawn.execution_scope);
    spawn.state.jobs.release_slot();

    let (status, body, err_note, duration_ms, ui_status) = match join {
        Ok(Ok(r)) => {
            let body = terminal_streaming_result_json(&r);
            let (ok, err_note) = terminal_stream_tool_status(&r);
            let status = job_status_from_terminal(&r, &spawn.cancel);
            let ui_status = if ok { "success" } else { "failed" };
            (status, body, err_note, Some(r.duration_ms), ui_status)
        }
        Ok(Err(err)) => {
            let cancelled = spawn.cancel.is_cancelled();
            let status = if cancelled {
                JobStatus::Cancelled
            } else {
                JobStatus::Failed
            };
            let note = err.to_string();
            log::warn!(
                "terminal background command failed job_id={job_id} conversation_id={}: {err:#}",
                spawn.conversation_id
            );
            (
                status,
                format!("ERROR: {note}"),
                Some(note),
                None,
                "failed",
            )
        }
        Err(err) => {
            let cancelled = spawn.cancel.is_cancelled();
            let status = if cancelled {
                JobStatus::Cancelled
            } else {
                JobStatus::Failed
            };
            let note = format!("终端执行线程异常: {err}");
            log::warn!(
                "terminal background join failed job_id={job_id} conversation_id={}: {err}",
                spawn.conversation_id
            );
            (
                status,
                format!("ERROR: {note}"),
                Some(note),
                None,
                "failed",
            )
        }
    };

    spawn
        .state
        .jobs
        .finish(&job_id, status, Some(body.clone()), err_note.clone());
    emit_and_persist_host_tool_finish(
        &spawn.stream,
        &spawn.conversation_id,
        &spawn.message_id,
        &spawn.tool_call_id,
        ui_status,
        body,
        err_note.as_deref(),
        duration_ms,
        spawn.host_trace_id.as_deref(),
        spawn.host_scoped_message_id.as_deref(),
    );
    emit_background_jobs(
        &spawn.stream,
        &spawn.conversation_id,
        spawn
            .state
            .jobs
            .running_count_for_conversation(&spawn.conversation_id),
    );
    log::info!(
        "terminal background finished job_id={job_id} conversation_id={} status={}",
        spawn.conversation_id,
        status.as_str()
    );
}
