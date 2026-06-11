//! Shared tool execution pass after envelope validation (lead single-agent and sub-agent).

use crate::agent_instance_scope::AgentInstanceScope;
use crate::agents::computer::ComputerTierGuard;
use crate::agents::{AgentDef, AgentProfile, AgentRunResult, AgentTask, FileToolLeadProfileGuard};
use crate::llm_token_stats::{ChatLlmTokenSession, ConversationLlmStats};
use crate::models::{AgentTrace, ChatMessage, StreamEvent, ToolCall};
use crate::provider::OpenAIProvider;
use crate::tools::normalize_tool_invoke_name;
use crate::tools::registry_tool_in_allow_list;
use crate::tools::parse_tool_call_arguments;
use crate::tools::terminal::{run_terminal_command_streaming, terminal_stream_tool_status};
use crate::tools::web_search::{
    dispatch_to_tool_json_async, WebSearchDispatchContext, WebSearchInvokeContext,
    WebSearchTokenSink,
};
use crate::tools::media_generate::{
    dispatch_media_generate_async, MediaGenerateDispatchContext,
};
use anyhow::{anyhow, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::app_state::AppState;
use super::emit::{emit, emit_task_board_updated, trace_id_opt};
use super::session_budget::SessionToolBudget;
use crate::task_board::{
    inject_host_task_board_conversation_id, is_task_board_tool_name, maybe_trim_after_tool_pass,
    task_board_call_is_checkpoint, TaskBoardTrimHook,
};
use super::util::{
    append_assistant_tool_raw_output, desktop_tool_failure_note, patch_assistant_tool_call_display,
    tool_display_stream_fields, truncate_str,
};
use super::StreamTx;

/// Outcome of executing a non-empty validated tool batch for one assistant turn.
#[derive(Debug)]
pub(super) enum ToolPassResult {
    /// Sub-agent: tool pass ended without executing tools (legacy exit; prefer `FinishRun` in sub loop).
    SubFinished(AgentRunResult),
    /// Lone successful [`ToolEntry::final_reply`] tool — host delivers output as the final message.
    FinalReplyComplete(String),
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
    pub run_id: &'a str,
    pub allow_agents: &'a [String],
    pub enabled_skill_ids: &'a mut Vec<String>,
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub file_tool_lead_for_invoke: AgentProfile,
    pub lead_agent_id: &'a str,
}

pub(super) struct SubToolPassConfig<'a> {
    pub def: &'a AgentDef,
    pub task: &'a AgentTask,
    pub allowed_tools: &'a [String],
    pub allow_agents: &'a [String],
    pub instance_scope: &'a AgentInstanceScope,
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub accumulated_content: String,
    pub accumulated_reasoning: String,
    pub reasoning_in_messages: bool,
    pub trace_id: String,
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
    mut sub: Option<SubToolPassConfig<'_>>,
    task_board_trim: Option<TaskBoardTrimHook<'_>>,
) -> Result<ToolPassResult> {
    let sub_trace_id = sub.as_ref().map(|s| s.trace_id.clone());
    let mut any_executed = false;
    let mut task_board_succeeded = false;
    let mut final_reply_output: Option<String> = None;
    for tc in final_tool_calls {
        if cancel.is_cancelled() {
            if let Some(consumed) = consumed_single {
                tool_budget.sync_out(consumed);
            }
            state.computer_state.mark_cancelled(conversation_id);
            return Err(anyhow!("已停止生成"));
        }

        let args_value = parse_tool_call_arguments(&tc.arguments);
        let (tool_id, args_value) = normalize_tool_invoke_name(&tc.name, args_value);
        let args_value = inject_host_task_board_conversation_id(
            &tool_id,
            args_value,
            conversation_id,
            task_board_store_key,
            history,
        );
        let mut args_value = args_value;
        if tool_id == "channel_message" {
            if let Some(obj) = args_value.as_object_mut() {
                obj.insert(
                    "_conversation_id".to_string(),
                    serde_json::Value::String(conversation_id.to_string()),
                );
            }
        }
        if tool_id.is_empty() {
            let err = "工具名为空：请检查 <tool_name>（例如 mouse_click_index、input、response）。";
            emit(
                &stream,
                StreamEvent::ToolCallStatus {
                    message_id: message_id.clone(),
                    tool_call_id: tc.id.clone(),
                    status: "failed".into(),
                    result: None,
                    error: Some(err.to_string()),
                    duration_ms: Some(0),
                    display_label: None,
                    display_summary: None,
                    trace_id: trace_id_opt(sub_trace_id.as_deref()),
                },
            );
            super::util::push_tool_result(history, conversation_id, &tc.id, &format!("ERROR: {err}"));
            any_executed = true;
            continue;
        }

        if let Some(sub_cfg) = sub.as_ref() {
            if !registry_tool_in_allow_list(sub_cfg.allowed_tools, &tool_id) {
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
                    display_label: None,
                    display_summary: None,
                    trace_id: trace_id_opt(sub_trace_id.as_deref()),
                    },
                );
                super::util::push_tool_result(history, conversation_id, &tc.id, &format!("ERROR: {err}"));
                any_executed = true;
                continue;
            }
        }

        if !run_approval_gate(
            &stream,
            &state,
            conversation_id,
            history,
            tool_approval_mode,
            &message_id,
            tc,
            &tool_id,
            &args_value,
            &cancel,
            sub_trace_id.as_deref(),
        )
        .await?
        {
            any_executed = true;
            continue;
        }

        let display = state.tools.format_display(&tc.name, &args_value);
        patch_assistant_tool_call_display(history, &message_id, &tc.id, &display);
        let (display_label, display_summary) = tool_display_stream_fields(&display);
        emit(
            &stream,
            StreamEvent::ToolCallStatus {
                message_id: message_id.clone(),
                tool_call_id: tc.id.clone(),
                status: "running".into(),
                result: None,
                error: None,
                duration_ms: None,
                display_label,
                display_summary,
                trace_id: trace_id_opt(sub_trace_id.as_deref()),
            },
        );
        if tool_id.starts_with("captcha_verify_") {
            emit(
                &stream,
                StreamEvent::ToolCallStatus {
                    message_id: message_id.clone(),
                    tool_call_id: tc.id.clone(),
                    status: "running".into(),
                    result: Some("识别中...".into()),
                    error: None,
                    duration_ms: None,
                    display_label: None,
                    display_summary: Some("识别中...".into()),
                    trace_id: trace_id_opt(sub_trace_id.as_deref()),
                },
            );
        }
        stats.record_tool_invocation();
        let started = Instant::now();

        let exec = execute_tool_invocation(
            &stream,
            &state,
            provider,
            conversation_id,
            task_board_store_key,
            &message_id,
            history,
            tc,
            &tool_id,
            args_value.clone(),
            lead.as_mut(),
            sub.as_mut(),
            &cancel,
            stats,
        )
        .await;

        let duration = started.elapsed().as_millis() as u64;
        let tool_ok = match &exec {
            Ok((_, ok, _)) => *ok,
            Err(_) => false,
        };
        let final_reply_candidate = exec.as_ref().ok().and_then(|(out, ok, _)| {
            state
                .tools
                .should_finalize_after_success(final_tool_calls, &tool_id, *ok)
                .then(|| out.clone())
        });
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
            sub_trace_id.as_deref(),
        )
        .await;
        if let Some(out) = final_reply_candidate {
            final_reply_output = Some(out);
        }
        if tool_ok && is_task_board_tool_name(&tool_id) {
            if task_board_call_is_checkpoint(&tool_id, &args_value) {
                task_board_succeeded = true;
            }
            let doc = state.task_board_store.document(task_board_store_key);
            let anchor_message_id = if crate::task_board::is_child_store_key(task_board_store_key) {
                Some(message_id.clone())
            } else {
                state.get_main_task_board_anchor(conversation_id, task_board_store_key)
            };
            emit_task_board_updated(
                &stream,
                conversation_id,
                task_board_store_key,
                anchor_message_id,
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
    if let Some(output) = final_reply_output {
        return Ok(ToolPassResult::FinalReplyComplete(output));
    }
    Ok(ToolPassResult::RanTools)
}

async fn run_approval_gate(
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
    super::util::push_tool_result(history, conversation_id, &tc.id, &err);
    Ok(false)
}

async fn execute_tool_invocation(
    stream: &StreamTx,
    state: &AppState,
    provider: &OpenAIProvider,
    conversation_id: &str,
    parent_task_board_store_key: &str,
    message_id: &str,
    history: &[ChatMessage],
    tc: &ToolCall,
    tool_id: &str,
    args_value: serde_json::Value,
    mut lead: Option<&mut LeadToolPassConfig<'_>>,
    sub: Option<&mut SubToolPassConfig<'_>>,
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
            sub.map(|s| s.trace_id.clone()),
        )
        .await;
    }

    if tool_id == "web_search" {
        let invoke = if sub.as_ref().map(|s| s.def.id.as_str()) == Some("research") {
            WebSearchInvokeContext::ResearchSubAgent {
                history,
                exclude_message_id: message_id,
            }
        } else {
            WebSearchInvokeContext::Tool
        };
        let token_sink = match stats {
            ToolInvocationStats::TokenSession(s) => WebSearchTokenSink::Lead(s),
            ToolInvocationStats::Conversation(s) => WebSearchTokenSink::Sub {
                stats: s,
                scope: &sub
                    .as_ref()
                    .ok_or_else(|| anyhow!("web_search sub scope missing"))?
                    .instance_scope,
            },
        };
        let agent_id = sub
            .as_ref()
            .map(|s| s.def.id.as_str())
            .or_else(|| lead.as_ref().map(|l| l.lead_agent_id));
        let trace_id = sub.as_ref().map(|s| s.trace_id.clone());
        return dispatch_to_tool_json_async(WebSearchDispatchContext {
            settings: &provider.settings,
            agent_id,
            args: args_value,
            cancel: cancel.clone(),
            stream: stream.clone(),
            message_id: message_id.to_string(),
            tool_call_id: tc.id.clone(),
            history,
            exclude_message_id: message_id,
            invoke,
            token_sink,
            trace_id,
        })
        .await;
    }

    if tool_id == "image_generate" || tool_id == "video_generate" {
        let run_id = if let Some(lead_cfg) = lead.as_ref() {
            lead_cfg.run_id
        } else if let Some(sub_cfg) = sub.as_ref() {
            sub_cfg.instance_scope.run_id.as_str()
        } else {
            return Err(anyhow!("media generation requires lead or sub scope"));
        };
        return dispatch_media_generate_async(MediaGenerateDispatchContext {
            settings: &provider.settings,
            conversation_id,
            run_id,
            tool_id,
            args: args_value,
            cancel: cancel.clone(),
            stream: stream.clone(),
            message_id: message_id.to_string(),
            tool_call_id: tc.id.clone(),
        })
        .await;
    }

    if tool_id == "run_subagent" {
        let llm_stats = match stats {
            ToolInvocationStats::TokenSession(s) => &mut s.stats,
            ToolInvocationStats::Conversation(s) => s,
        };
        if let Some(lead_cfg) = lead {
            return super::run_subagent_delegation::run_subagent_delegation(
                stream,
                state,
                provider,
                conversation_id,
                parent_task_board_store_key,
                message_id,
                &tc.id,
                args_value,
                lead_cfg.run_id,
                lead_cfg.allow_agents,
                lead_cfg.enabled_skill_ids.as_slice(),
                lead_cfg.agent_trace,
                cancel,
                llm_stats,
            )
            .await;
        }
        if let Some(sub_cfg) = sub {
            let empty_skills: &[String] = &[];
            return super::run_subagent_delegation::run_subagent_delegation(
                stream,
                state,
                provider,
                conversation_id,
                parent_task_board_store_key,
                message_id,
                &tc.id,
                args_value,
                &sub_cfg.instance_scope.run_id,
                sub_cfg.allow_agents,
                empty_skills,
                sub_cfg.agent_trace,
                cancel,
                llm_stats,
            )
            .await;
        }
    }

    if tool_id == "skill_import" {
        let auto_enable = args_value
            .get("auto_enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let out = state.tools.invoke(tool_id, args_value.clone())?;
        let result: crate::models::SkillImportResult = serde_json::from_str(&out)
            .map_err(|e| anyhow!("skill_import 结果解析失败: {e}"))?;
        let imported_ids: Vec<String> = result.imported.iter().map(|s| s.id.clone()).collect();
        log::info!(
            "skill_import: conversation_id={conversation_id} imported={} skipped={}",
            imported_ids.len(),
            result.skipped.len()
        );
        let enabled_ids = if auto_enable {
            if let Some(lead_cfg) = lead.as_mut() {
                for id in &imported_ids {
                    if !lead_cfg.enabled_skill_ids.contains(id) {
                        lead_cfg.enabled_skill_ids.push(id.clone());
                    }
                }
                let ids = lead_cfg.enabled_skill_ids.clone();
                let mut user = state.load_user_settings();
                user.enabled_skill_ids = ids.clone();
                if let Err(err) = state.save_user_settings(&user) {
                    log::warn!("skill_import: persist enabled_skill_ids failed: {err}");
                }
                Some(ids)
            } else {
                None
            }
        } else {
            None
        };
        emit(
            stream,
            StreamEvent::SkillsUpdated {
                conversation_id: conversation_id.to_string(),
                imported_ids: imported_ids.clone(),
                enabled_ids,
            },
        );
        return Ok((out, true, None));
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
    trace_id: Option<String>,
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
    let trace_id_for_terminal = trace_id_opt(trace_id.as_deref());
    let join = tokio::task::spawn_blocking(move || {
        run_terminal_command_streaming(
            args_value,
            move |output| {
                let _ = stream_for_terminal.send(StreamEvent::TerminalOutputDelta {
                    message_id: msg_id_for_stream.clone(),
                    tool_call_id: tc_id_for_stream.clone(),
                    output: output.to_string(),
                    trace_id: trace_id_for_terminal.clone(),
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
                "elevationDenied": r.elevation_denied,
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
    trace_id: Option<&str>,
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
                    display_label: None,
                    display_summary: None,
                    trace_id: trace_id_opt(trace_id),
                },
            );
            append_assistant_tool_raw_output(
                history,
                message_id,
                &tc.name,
                &tc.id,
                args_for_desktop_log,
                &out,
            );
            super::util::push_tool_result(history, conversation_id, &tc.id, &out);
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
                history,
                message_id,
                &tc.name,
                &tc.id,
                args_for_desktop_log,
                &error_out,
            );
            super::util::push_tool_result(history, conversation_id, &tc.id, &error_out);
        }
    }
}
