//! Shared tool execution pass after envelope validation (lead single-agent and sub-agent).

use crate::agents::computer::ComputerTierGuard;
use crate::agents::{AgentDef, AgentProfile, AgentRunResult, AgentTask, FileToolLeadProfileGuard};
use crate::llm_token_stats::{ChatLlmTokenSession, ConversationLlmStats};
use crate::models::{AgentTrace, ChatMessage, Role, StreamEvent, ToolCall};
use crate::provider::OpenAIProvider;
use crate::tools::merge_tool_method_from_qualified_name;
use crate::tools::parse_tool_call_arguments;
use crate::tools::response::response_text_from_args;
use crate::tools::terminal::{run_terminal_command_streaming, terminal_stream_tool_status};
use anyhow::{anyhow, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::app_state::AppState;
use super::emit::{emit, emit_task_board_updated};
use super::session_budget::SessionToolBudget;
use crate::task_board::{
    inject_host_task_board_conversation_id, maybe_trim_after_tool_pass,
    task_board_call_is_checkpoint, TaskBoardTrimHook,
};
use super::util::{desktop_tool_failure_note, tool_result_msg, truncate_str};
use super::StreamTx;

/// Outcome of executing a non-empty validated tool batch for one assistant turn.
#[derive(Debug)]
pub(super) enum ToolPassResult {
    /// Lead: `response` tool ended the turn successfully (`tool_budget` already synced).
    LeadFinished,
    /// Sub-agent: `response` tool ended with handoff payload.
    SubFinished(AgentRunResult),
    /// No tool ran to completion in a way that consumes a round (synced out for lead).
    NoopExit,
    /// At least one tool produced results; caller should record a tool cycle and check budget.
    RanTools,
}

pub(super) enum ToolInvocationStats<'a> {
    TokenSession(&'a mut ChatLlmTokenSession),
    Conversation(&'a mut ConversationLlmStats),
}

impl ToolInvocationStats<'_> {
    fn record_tool_invocation(&mut self) {
        match self {
            ToolInvocationStats::TokenSession(s) => s.stats.record_tool_invocation(),
            ToolInvocationStats::Conversation(s) => s.record_tool_invocation(),
        }
    }
}

pub(super) struct LeadToolPassConfig<'a> {
    pub allow_agents: &'a [String],
    pub enabled_skill_ids: &'a [String],
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub raw_content_buf: &'a str,
    pub file_tool_lead_for_invoke: AgentProfile,
}

pub(super) struct SubToolPassConfig<'a> {
    pub def: &'a AgentDef,
    pub task: &'a AgentTask,
    pub allowed_tools: &'a [String],
    pub round_message_id: &'a str,
    pub accumulated_content: String,
    pub accumulated_reasoning: String,
    pub reasoning_in_messages: bool,
}

pub(super) async fn run_agent_tool_pass(
    stream: StreamTx,
    state: &AppState,
    conversation_id: &str,
    message_id: String,
    history: &mut Vec<ChatMessage>,
    tool_approval_mode: &str,
    tool_budget: &mut SessionToolBudget,
    consumed_single: Option<&mut u32>,
    cancel: CancellationToken,
    provider: &OpenAIProvider,
    task_board_store_key: &str,
    stats: &mut ToolInvocationStats<'_>,
    final_tool_calls: &[ToolCall],
    mut lead: Option<LeadToolPassConfig<'_>>,
    sub: Option<SubToolPassConfig<'_>>,
    task_board_trim: Option<TaskBoardTrimHook<'_>>,
) -> Result<ToolPassResult> {
    let mut any_executed = false;
    let mut task_board_succeeded = false;
    for tc in final_tool_calls {
        if cancel.is_cancelled() {
            if let Some(consumed) = consumed_single {
                tool_budget.sync_out(consumed);
            }
            state.computer_state.mark_cancelled(conversation_id);
            return Err(anyhow!("已停止生成"));
        }

        let args_value = parse_tool_call_arguments(&tc.arguments);
        let (mut tool_id, args_value) = merge_tool_method_from_qualified_name(&tc.name, args_value);
        tool_id = tool_id.trim().to_string();
        let args_value = inject_host_task_board_conversation_id(
            &tool_id,
            args_value,
            task_board_store_key,
            history,
        );
        if tool_id.is_empty() {
            let err = "工具名为空：请检查 <tool_name>（例如 mouse:click_index、composite_action、response）。";
            emit(
                &stream,
                StreamEvent::ToolCallStatus {
                    message_id: message_id.clone(),
                    tool_call_id: tc.id.clone(),
                    status: "failed".into(),
                    result: None,
                    error: Some(err.to_string()),
                    duration_ms: Some(0),
                },
            );
            history.push(tool_result_msg(&tc.id, &format!("ERROR: {err}")));
            any_executed = true;
            continue;
        }

        if tool_id == "response" {
            return handle_response_tool(
                &stream,
                history,
                tool_budget,
                consumed_single,
                &message_id,
                tc,
                &args_value,
                lead.as_ref(),
                sub.as_ref(),
            ); // response path does not need mutable lead
        }

        if let Some(sub_cfg) = sub.as_ref() {
            if tool_id == "run_subagent" {
                let err = "子 Agent 内不可再次调用 run_subagent。";
                emit(
                    &stream,
                    StreamEvent::ToolCallStatus {
                        message_id: message_id.clone(),
                        tool_call_id: tc.id.clone(),
                        status: "failed".into(),
                        result: None,
                        error: Some(err.to_string()),
                        duration_ms: None,
                    },
                );
                history.push(tool_result_msg(&tc.id, &format!("ERROR: {err}")));
                any_executed = true;
                continue;
            }
            if !sub_cfg.allowed_tools.contains(&tool_id) {
                let err = format!(
                    "Agent {} 不允许调用工具: {}",
                    sub_cfg.def.id, tc.name
                );
                emit(
                    &stream,
                    StreamEvent::ToolCallStatus {
                        message_id: message_id.clone(),
                        tool_call_id: tc.id.clone(),
                        status: "failed".into(),
                        result: None,
                        error: Some(err.clone()),
                        duration_ms: None,
                    },
                );
                history.push(tool_result_msg(&tc.id, &format!("ERROR: {err}")));
                any_executed = true;
                continue;
            }
        }

        if !run_approval_gate(
            &stream,
            &state,
            history,
            tool_approval_mode,
            &message_id,
            tc,
            &tool_id,
            &args_value,
            &cancel,
        )
        .await?
        {
            any_executed = true;
            continue;
        }

        emit(
            &stream,
            StreamEvent::ToolCallStatus {
                message_id: message_id.clone(),
                tool_call_id: tc.id.clone(),
                status: "running".into(),
                result: None,
                error: None,
                duration_ms: None,
            },
        );
        stats.record_tool_invocation();
        let started = Instant::now();

        let exec = execute_tool_invocation(
            &stream,
            &state,
            provider,
            conversation_id,
            &message_id,
            tc,
            &tool_id,
            args_value.clone(),
            lead.as_mut(),
            sub.as_ref(),
            &cancel,
            stats,
        )
        .await;

        let duration = started.elapsed().as_millis() as u64;
        let tool_ok = match &exec {
            Ok((_, ok, _)) => *ok,
            Err(_) => false,
        };
        record_tool_exec_outcome(
            &stream,
            &state,
            history,
            conversation_id,
            &message_id,
            tc,
            &tool_id,
            &args_value,
            exec,
            duration,
        )
        .await;
        if tool_ok && task_board_call_is_checkpoint(&tool_id, &args_value) {
            task_board_succeeded = true;
            let doc = state.task_board_store.document(task_board_store_key);
            let host_cid = args_value
                .get("_conversation_id")
                .and_then(|v| v.as_str())
                .unwrap_or(conversation_id);
            emit_task_board_updated(
                &stream,
                host_cid,
                task_board_store_key,
                doc.to_value(),
            );
        }
        any_executed = true;
    }

    if let Some(hook) = task_board_trim.as_ref() {
        maybe_trim_after_tool_pass(history, hook, task_board_succeeded);
    }

    if !any_executed {
        if let Some(consumed) = consumed_single {
            tool_budget.sync_out(consumed);
            return Ok(ToolPassResult::NoopExit);
        }
        if let Some(sub_cfg) = sub {
            return Ok(ToolPassResult::SubFinished(super::agent_post_stream::sub_agent_run_result(
                &sub_cfg.task.id,
                sub_cfg.def,
                sub_cfg.accumulated_content,
                sub_cfg.reasoning_in_messages,
                sub_cfg.accumulated_reasoning,
            )));
        }
        return Ok(ToolPassResult::NoopExit);
    }
    Ok(ToolPassResult::RanTools)
}

fn handle_response_tool(
    stream: &StreamTx,
    history: &mut Vec<ChatMessage>,
    tool_budget: &mut SessionToolBudget,
    consumed_single: Option<&mut u32>,
    message_id: &str,
    tc: &ToolCall,
    args_value: &serde_json::Value,
    lead: Option<&LeadToolPassConfig<'_>>,
    sub: Option<&SubToolPassConfig<'_>>,
) -> Result<ToolPassResult> {
    let message = response_text_from_args(args_value).unwrap_or("");

    if !message.is_empty() {
        emit(
            stream,
            StreamEvent::Delta {
                message_id: message_id.to_string(),
                text: message.to_string(),
            },
        );
    }

    emit(
        stream,
        StreamEvent::ToolCallStatus {
            message_id: message_id.to_string(),
            tool_call_id: tc.id.clone(),
            status: "success".into(),
            result: Some("已回复用户".into()),
            error: None,
            duration_ms: Some(0),
        },
    );

    if let Some(lead_cfg) = lead {
        let assistant_id = message_id;
        let mut wire_thoughts: Option<String> = None;
        let mut wire_headline: Option<String> = None;
        if let Some(last) = history.last_mut() {
            if last.id == assistant_id && matches!(last.role, Role::Assistant) {
                last.content = message.to_string();
                last.tool_calls = None;
                last.status = "completed".into();
                wire_thoughts = last.thoughts.clone();
                wire_headline = last.headline.clone();
            }
        }
        emit(
            stream,
            StreamEvent::MessageEnd {
                message_id: assistant_id.to_string(),
                content: Some(message.to_string()),
                raw_content: if lead_cfg.raw_content_buf.is_empty() {
                    None
                } else {
                    Some(lead_cfg.raw_content_buf.to_string())
                },
                thoughts: wire_thoughts,
                headline: wire_headline,
            },
        );
        if let Some(consumed) = consumed_single {
            tool_budget.sync_out(consumed);
        }
        return Ok(ToolPassResult::LeadFinished);
    }

    if let Some(sub_cfg) = sub {
        if let Some(last) = history.last_mut() {
            if last.id == sub_cfg.round_message_id && matches!(last.role, Role::Assistant) {
                last.content = message.to_string();
                last.tool_calls = None;
                last.status = "completed".into();
            }
        }
        return Ok(ToolPassResult::SubFinished(
            super::agent_post_stream::sub_agent_run_result(
                &sub_cfg.task.id,
                sub_cfg.def,
                message.to_string(),
                sub_cfg.reasoning_in_messages,
                sub_cfg.accumulated_reasoning.clone(),
            ),
        ));
    }

    Err(anyhow!("response tool without lead or sub config"))
}

async fn run_approval_gate(
    stream: &StreamTx,
    state: &AppState,
    history: &mut Vec<ChatMessage>,
    tool_approval_mode: &str,
    message_id: &str,
    tc: &ToolCall,
    tool_id: &str,
    args_value: &serde_json::Value,
    cancel: &CancellationToken,
) -> Result<bool> {
    let requires_approval = tool_approval_mode == "manual"
        && state
            .tools
            .tool_invocation_needs_approval(tool_id, args_value);
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
        },
    );
    history.push(tool_result_msg(&tc.id, &err));
    Ok(false)
}

async fn execute_tool_invocation(
    stream: &StreamTx,
    state: &AppState,
    provider: &OpenAIProvider,
    conversation_id: &str,
    message_id: &str,
    tc: &ToolCall,
    tool_id: &str,
    args_value: serde_json::Value,
    lead: Option<&mut LeadToolPassConfig<'_>>,
    sub: Option<&SubToolPassConfig<'_>>,
    cancel: &CancellationToken,
    stats: &mut ToolInvocationStats<'_>,
) -> Result<(String, bool, Option<String>), anyhow::Error> {
    if tool_id == "terminal" {
        return run_terminal_tool(
            stream,
            state,
            conversation_id,
            message_id,
            tc,
            args_value,
            cancel,
        )
        .await;
    }

    if tool_id == "run_subagent" {
        if let Some(lead_cfg) = lead {
            let llm_stats = match stats {
                ToolInvocationStats::TokenSession(s) => &mut s.stats,
                ToolInvocationStats::Conversation(s) => s,
            };
            return super::run_subagent_delegation::run_subagent_delegation(
                stream,
                state,
                provider,
                conversation_id,
                message_id,
                args_value,
                lead_cfg.allow_agents,
                lead_cfg.enabled_skill_ids,
                lead_cfg.agent_trace,
                cancel,
                llm_stats,
            )
            .await;
        }
    }

    let file_profile = lead
        .map(|l| l.file_tool_lead_for_invoke.clone())
        .or_else(|| sub.map(|s| s.def.profile.clone()))
        .unwrap_or(AgentProfile::General);
    let _file_tool_profile_guard = FileToolLeadProfileGuard::enter(file_profile.clone());
    let _tier_guard = if file_profile == AgentProfile::Computer {
        Some(ComputerTierGuard::enter(
            state.computer_state.tier_for_conversation(conversation_id),
        ))
    } else {
        None
    };
    state
        .tools
        .invoke(tool_id, args_value)
        .map(|out| (out, true, None))
}

async fn run_terminal_tool(
    stream: &StreamTx,
    state: &AppState,
    conversation_id: &str,
    message_id: &str,
    tc: &ToolCall,
    args_value: serde_json::Value,
    cancel: &CancellationToken,
) -> Result<(String, bool, Option<String>), anyhow::Error> {
    let cancel_terminal = cancel.clone();
    let abort_flag = Arc::new(AtomicBool::new(false));
    {
        let mut m = state.terminal_run_abort.lock();
        if let Some(old) = m.insert(conversation_id.to_string(), abort_flag.clone()) {
            old.store(true, Ordering::SeqCst);
        }
    }
    let cleanup_id = conversation_id.to_string();
    let msg_id_for_stream = message_id.to_string();
    let tc_id_for_stream = tc.id.clone();
    let stream_for_terminal = stream.clone();
    let join = tokio::task::spawn_blocking(move || {
        run_terminal_command_streaming(
            args_value,
            move |output| {
                let _ = stream_for_terminal.send(StreamEvent::TerminalOutputDelta {
                    message_id: msg_id_for_stream.clone(),
                    tool_call_id: tc_id_for_stream.clone(),
                    output: output.to_string(),
                });
            },
            Some(cancel_terminal),
            Some(abort_flag),
        )
        .map(|r| {
            let (ok, err_note) = terminal_stream_tool_status(&r);
            let body = serde_json::json!({
                "exitCode": r.exit_code,
                "success": r.success,
                "timedOut": r.timed_out,
                "cancelled": r.cancelled,
                "runAborted": r.run_aborted,
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
    state.terminal_run_abort.lock().remove(&cleanup_id);
    join.map_err(|e| anyhow!("终端执行线程异常: {e}"))?
}

async fn record_tool_exec_outcome(
    stream: &StreamTx,
    state: &AppState,
    history: &mut Vec<ChatMessage>,
    conversation_id: &str,
    message_id: &str,
    tc: &ToolCall,
    tool_id: &str,
    args_for_desktop_log: &serde_json::Value,
    exec: Result<(String, bool, Option<String>), anyhow::Error>,
    duration: u64,
) {
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
                },
            );
            history.push(tool_result_msg(&tc.id, &out));
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
                },
            );
            history.push(tool_result_msg(&tc.id, &format!("ERROR: {err}")));
        }
    }
}
