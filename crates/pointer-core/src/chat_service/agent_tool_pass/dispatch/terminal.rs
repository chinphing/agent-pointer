//! Terminal tool streaming execution.

use crate::chat_service::job_supervisor::{
    AwaitMode, JobCaller, JobKind, JobKindTerminal, JobStatus, JobSupervisor,
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
    effective_block_until_ms, parse_block_until_ms, run_terminal_command_streaming,
    terminal_requests_elevation, terminal_stream_tool_status, terminal_streaming_result_json,
    TerminalInputHooks, TerminalInputResolution, TerminalNeedsInputPrompt, TerminalStreamingResult,
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
    // Shared with file tools so both always agree on the directory.
    crate::workspace_resolve::resolve_tool_workspace(
        conversation_id,
        None,
        &from_settings,
        "terminal",
    )
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
    // `action` is the schema-required intent field, so it owns the job title.
    // `label` is the pre-rename key — history rows of old sessions still carry
    // it — so it is a fallback and must never win over `action`. Same precedence
    // as the tool-row summary in `tools::display` (terminal arm).
    let label = args
        .get("action")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            args.get("label")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        });
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

fn job_status_from_terminal(
    r: &TerminalStreamingResult,
    job_cancel: &CancellationToken,
) -> JobStatus {
    let (ok, _) = terminal_stream_tool_status(r);
    if r.cancelled || job_cancel.is_cancelled() {
        JobStatus::Cancelled
    } else if ok {
        JobStatus::Completed
    } else if r.exit_code.is_none()
        && !r.timed_out
        && !r.elevation_denied
        && !(r.agent_retry_forbidden && r.needs_input_likely && !r.user_input_provided)
    {
        // 没有退出码 = 进程被信号终止（例如自己 `pkill` 掉刚起的后台命令）。
        // 那不是「命令失败」，记成 Cancelled：既不推送「后台任务失败」，
        // 也让 `job status` 显示得符合实际。
        JobStatus::Cancelled
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
    run_id: &str,
    owner_chain: &[String],
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
            run_id,
            owner_chain,
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
    run_id: &str,
    owner_chain: &[String],
) -> ToolExecResult {
    let job_cancel = CancellationToken::new();
    let (command, label) = terminal_job_title(&args_value);
    let kind = JobKind::Terminal(JobKindTerminal {
        tool_call_id: tc.id.clone(),
        message_id: message_id.to_string(),
        command,
        label,
    });
    let job_id = state.jobs.register(
        conversation_id,
        kind,
        job_cancel.clone(),
        run_id,
        owner_chain.to_vec(),
    );
    emit_background_jobs(stream, conversation_id, &state.jobs);
    log::info!(
        "terminal background spawn job_id={job_id} conversation_id={conversation_id} tool_call_id={} block_until_ms={block_until_ms} nested={}",
        tc.id,
        trace_id.as_deref().filter(|s| !s.is_empty()).is_some()
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

    let handle = background_job_handle_json(&job_id, JobStatus::Running, "terminal", None, None);
    // A large `blockUntilMs` must never park the parent turn: the job keeps
    // running either way, so cap the in-call wait and hand back the handle.
    let wait_ms = effective_block_until_ms(block_until_ms);
    if wait_ms == 0 {
        log::info!(
            "terminal background detached immediately job_id={job_id} conversation_id={conversation_id}"
        );
        return Ok((handle, true, None));
    }
    if wait_ms != block_until_ms {
        log::warn!(
            "terminal background wait clamped job_id={job_id} conversation_id={conversation_id} requested_ms={block_until_ms} clamped_ms={wait_ms}"
        );
    }

    let slot_cap = JobSupervisor::slot_cap_from(
        ParallelLimits::from_settings(&state.effective_settings()).max_parallel_sub_agents,
    );
    log::info!(
        "terminal background waiting job_id={job_id} conversation_id={conversation_id} block_until_ms={block_until_ms} wait_ms={wait_ms}"
    );
    let result = match state
        .jobs
        .await_jobs(
            conversation_id,
            terminal_self_caller(owner_chain),
            Some(vec![job_id.clone()]),
            AwaitMode::All,
            Some(Duration::from_millis(wait_ms)),
            parent_cancel,
            slot_cap,
        )
        .await
    {
        Ok(result) => result,
        Err(err) => {
            // Only reachable if the caller is not on the job's own chain; the
            // job keeps running, so hand the handle back instead of failing.
            log::warn!(
                "terminal background self-await rejected job_id={job_id} conversation_id={conversation_id}: {err}"
            );
            return Ok((handle, true, None));
        }
    };

    if let Some(item) = result.jobs.into_iter().next() {
        if item.status != "queued" && item.status != "running" {
            let _ = state.jobs.claim_if_unclaimed(&job_id);
            let body = item.content.unwrap_or_else(|| {
                background_job_handle_json(
                    &job_id,
                    status_from_wire(&item.status),
                    "terminal",
                    None,
                    None,
                )
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
            background_job_handle_json(
                &job_id,
                status_from_wire(item.status),
                "terminal",
                None,
                None,
            )
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

/// The issuer of a terminal job is the last link of its own chain (empty = lead),
/// which is also who may wait on it (`blockUntilMs`).
fn terminal_self_caller(owner_chain: &[String]) -> JobCaller<'_> {
    match owner_chain.last() {
        Some(instance_id) => JobCaller::Instance(instance_id.as_str()),
        None => JobCaller::Lead,
    }
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
    let Some(_lease) = spawn
        .state
        .jobs
        .acquire_root(&spawn.conversation_id, cap, &spawn.cancel)
        .await
    else {
        log::info!(
            "terminal background cancelled before slot job_id={job_id} conversation_id={}",
            spawn.conversation_id
        );
        let body = cancelled_terminal_json();
        crate::chat_service::deferred_token_finalize::finish_job_and_maybe_finalize_arc(
            &spawn.state,
            &job_id,
            JobStatus::Cancelled,
            Some(body.clone()),
            Some("cancelled".into()),
        )
        .await;
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
        emit_background_jobs(&spawn.stream, &spawn.conversation_id, &spawn.state.jobs);
        return;
    };
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
    drop(_lease);

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
            (status, format!("ERROR: {note}"), Some(note), None, "failed")
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
            (status, format!("ERROR: {note}"), Some(note), None, "failed")
        }
    };

    crate::chat_service::deferred_token_finalize::finish_job_and_maybe_finalize_arc(
        &spawn.state,
        &job_id,
        status,
        Some(body.clone()),
        err_note.clone(),
    )
    .await;
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
    emit_background_jobs(&spawn.stream, &spawn.conversation_id, &spawn.state.jobs);
    log::info!(
        "terminal background finished job_id={job_id} conversation_id={} status={}",
        spawn.conversation_id,
        status.as_str()
    );
}

#[cfg(test)]
mod tests {
    use super::{
        job_status_from_terminal, terminal_job_title, terminal_self_caller, InputClass,
        JobStatus, TerminalStreamingResult,
    };
    use crate::chat_service::job_supervisor::JobCaller;
    use serde_json::json;
    use tokio_util::sync::CancellationToken;

    /// The background-job title (idle push「后台任务已完成：…」) must read the
    /// intent, not the truncated raw command. `action` is the schema-required
    /// field, so it wins; `label` is only the pre-rename key for old rows.
    #[test]
    fn terminal_job_title_prefers_action_over_label() {
        let (command, label) = terminal_job_title(&json!({
            "command": "cargo test -p pointer-core --lib",
            "action": "运行 pointer-core 单元测试",
            "label": "旧的标题字段",
        }));
        assert_eq!(command, "cargo test -p pointer-core --lib");
        assert_eq!(label.as_deref(), Some("运行 pointer-core 单元测试"));
    }

    /// Old transcript rows have no `action`; without the fallback the push would
    /// degrade to the raw command.
    #[test]
    fn terminal_job_title_falls_back_to_label_for_old_rows() {
        let (command, label) = terminal_job_title(&json!({
            "command": "cargo test -p pointer-core --lib",
            "label": " 运行测试 ",
        }));
        assert_eq!(command, "cargo test -p pointer-core --lib");
        assert_eq!(label.as_deref(), Some("运行测试"));
    }

    /// Blank `action` is not an intent — it must not swallow the `label`
    /// fallback, and with neither present the title stays `None`.
    #[test]
    fn terminal_job_title_without_intent_is_none() {
        let (command, label) = terminal_job_title(&json!({
            "command": "sleep 1",
            "action": "   ",
            "label": "",
        }));
        assert_eq!(command, "sleep 1");
        assert_eq!(label, None);

        let (_, label) = terminal_job_title(&json!({ "command": "sleep 1" }));
        assert_eq!(label, None);
    }

    /// `blockUntilMs` waits as the job's own issuer, so the host self-wait keeps
    /// working after `job` became subtree-scoped.
    #[test]
    fn terminal_self_wait_uses_the_job_owner() {
        assert_eq!(terminal_self_caller(&[]), JobCaller::Lead);
        let chain = vec!["inst-coder".to_string(), "inst-terminal".to_string()];
        assert_eq!(
            terminal_self_caller(&chain),
            JobCaller::Instance("inst-terminal")
        );
    }

    fn streaming_result(exit_code: Option<i32>, success: bool) -> TerminalStreamingResult {
        TerminalStreamingResult {
            exit_code,
            success,
            timed_out: false,
            cancelled: false,
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
        }
    }

    /// 后台命令被 `pkill` 之类信号终止时没有退出码。那种情况不是「命令失败」，
    /// 记成 Cancelled，否则会把「我自己把它停了」当成「后台任务失败」推给用户。
    #[test]
    fn killed_terminal_job_is_cancelled_not_failed() {
        let token = CancellationToken::new();
        let killed = streaming_result(None, false);
        assert_eq!(
            job_status_from_terminal(&killed, &token),
            JobStatus::Cancelled
        );
    }

    /// 有退出码的非零退出仍然算失败——不能因为「没成功」就一律当取消。
    #[test]
    fn non_zero_exit_is_still_failed() {
        let token = CancellationToken::new();
        let failed = streaming_result(Some(1), false);
        assert_eq!(job_status_from_terminal(&failed, &token), JobStatus::Failed);
    }

    /// 超时被杀的进程同样没有退出码，但那是真失败，不能算取消。
    #[test]
    fn timed_out_terminal_job_stays_failed() {
        let token = CancellationToken::new();
        let mut timed_out = streaming_result(None, false);
        timed_out.timed_out = true;
        assert_eq!(
            job_status_from_terminal(&timed_out, &token),
            JobStatus::Failed
        );
    }
}
