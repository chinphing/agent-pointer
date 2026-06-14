//! Shared tool execution pass after envelope validation (lead single-agent and sub-agent).

mod approval;
mod dispatch;
mod outcome;
mod types;

pub(super) use types::{
    LeadToolPassConfig, SubToolPassConfig, ToolInvocationStats, ToolPassResult,
};

use crate::models::{StreamEvent, ToolCall};
use crate::provider::OpenAIProvider;
use crate::task_board::{
    inject_host_task_board_conversation_id, is_task_board_tool_name, maybe_trim_after_tool_pass,
    task_board_call_is_checkpoint, TaskBoardTrimHook,
};
use crate::tools::normalize_tool_invoke_name;
use crate::tools::parse_tool_call_arguments;
use crate::tools::registry_tool_in_allow_list;
use anyhow::{anyhow, Result};
use std::time::Instant;
use tokio_util::sync::CancellationToken;

use super::app_state::AppState;
use super::emit::{emit, emit_task_board_updated, trace_id_opt};
use super::session_budget::SessionToolBudget;
use super::util::{patch_assistant_tool_call_display, tool_display_stream_fields};
use super::StreamTx;

use approval::run_approval_gate;
use dispatch::execute_tool_invocation;
use outcome::record_tool_exec_outcome;

pub(super) async fn run_agent_tool_pass(
    stream: StreamTx,
    state: &AppState,
    conversation_id: &str,
    message_id: String,
    history: &mut Vec<crate::models::ChatMessage>,
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
    let persist_transcript = sub.is_none();
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

        if tool_id.is_empty() {
            emit_tool_failed(
                &stream,
                &message_id,
                tc,
                sub_trace_id.as_deref(),
                "工具名为空：请检查 <tool_name>（例如 mouse_click_index、input、response）。",
            );
            super::util::push_tool_result(
                history,
                conversation_id,
                &message_id,
                &tc.id,
                "ERROR: 工具名为空：请检查 <tool_name>（例如 mouse_click_index、input、response）。",
                persist_transcript,
            );
            any_executed = true;
            continue;
        }

        if let Some(sub_cfg) = sub.as_ref() {
            if !registry_tool_in_allow_list(sub_cfg.allowed_tools, &tool_id) {
                let err = format!("Agent {} 不允许调用工具: {}", sub_cfg.def.id, tc.name);
                emit_tool_failed(&stream, &message_id, tc, sub_trace_id.as_deref(), &err);
                super::util::push_tool_result(
                    history,
                    conversation_id,
                    &message_id,
                    &tc.id,
                    &format!("ERROR: {err}"),
                    persist_transcript,
                );
                any_executed = true;
                continue;
            }
        }

        if !run_approval_gate(
            &stream,
            state,
            conversation_id,
            history,
            tool_approval_mode,
            &message_id,
            tc,
            &tool_id,
            &args_value,
            &cancel,
            sub_trace_id.as_deref(),
            persist_transcript,
        )
        .await?
        {
            any_executed = true;
            continue;
        }

        emit_tool_running(
            &stream,
            state,
            history,
            &message_id,
            tc,
            &tool_id,
            &args_value,
            sub_trace_id.as_deref(),
        );
        stats.record_tool_invocation();
        let started = Instant::now();

        let exec = execute_tool_invocation(
            &stream,
            state,
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
        let tool_ok = matches!(&exec, Ok((_, ok, _)) if *ok);
        let final_reply_candidate = exec.as_ref().ok().and_then(|(out, ok, _)| {
            state
                .tools
                .should_finalize_after_success(final_tool_calls, &tool_id, *ok)
                .then(|| out.clone())
        });

        record_tool_exec_outcome(
            &stream,
            state,
            history,
            conversation_id,
            &message_id,
            tc,
            &tool_id,
            &args_value,
            exec,
            duration,
            sub_trace_id.as_deref(),
            persist_transcript,
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

    if persist_transcript {
        crate::conversation_transcript::flush_after_tool_pass(conversation_id, history);
    }

    if !any_executed {
        if let Some(consumed) = consumed_single {
            tool_budget.sync_out(consumed);
            return Ok(ToolPassResult::NoopExit);
        }
        if let Some(sub_cfg) = sub {
            return Ok(ToolPassResult::SubFinished(
                super::agent_post_stream::sub_agent_run_result(
                    &sub_cfg.task.id,
                    sub_cfg.def,
                    sub_cfg.accumulated_content,
                    sub_cfg.reasoning_in_messages,
                    sub_cfg.accumulated_reasoning,
                ),
            ));
        }
        return Ok(ToolPassResult::NoopExit);
    }
    if let Some(output) = final_reply_output {
        return Ok(ToolPassResult::FinalReplyComplete(output));
    }
    Ok(ToolPassResult::RanTools)
}

fn emit_tool_failed(
    stream: &StreamTx,
    message_id: &str,
    tc: &ToolCall,
    trace_id: Option<&str>,
    err: &str,
) {
    emit(
        stream,
        StreamEvent::ToolCallStatus {
            message_id: message_id.to_string(),
            tool_call_id: tc.id.clone(),
            status: "failed".into(),
            result: None,
            error: Some(err.to_string()),
            duration_ms: Some(0),
            display_label: None,
            display_summary: None,
            trace_id: trace_id_opt(trace_id),
        },
    );
}

fn emit_tool_running(
    stream: &StreamTx,
    state: &AppState,
    history: &mut Vec<crate::models::ChatMessage>,
    message_id: &str,
    tc: &ToolCall,
    tool_id: &str,
    args_value: &serde_json::Value,
    trace_id: Option<&str>,
) {
    let display = state.tools.format_display(&tc.name, args_value);
    patch_assistant_tool_call_display(history, message_id, &tc.id, &display);
    let (display_label, display_summary) = tool_display_stream_fields(&display);
    emit(
        stream,
        StreamEvent::ToolCallStatus {
            message_id: message_id.to_string(),
            tool_call_id: tc.id.clone(),
            status: "running".into(),
            result: None,
            error: None,
            duration_ms: None,
            display_label,
            display_summary,
            trace_id: trace_id_opt(trace_id),
        },
    );
    if tool_id.starts_with("captcha_verify_") {
        emit(
            stream,
            StreamEvent::ToolCallStatus {
                message_id: message_id.to_string(),
                tool_call_id: tc.id.clone(),
                status: "running".into(),
                result: Some("识别中...".into()),
                error: None,
                duration_ms: None,
                display_label: None,
                display_summary: Some("识别中...".into()),
                trace_id: trace_id_opt(trace_id),
            },
        );
    }
}
