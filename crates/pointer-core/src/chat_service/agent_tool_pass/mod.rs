//! Shared tool execution pass after envelope validation (lead single-agent and sub-agent).

mod approval;
mod batch;
mod dispatch;
mod outcome;
mod types;

pub(super) use types::{
    LeadSingleToolPassRequest, LeadToolPassConfig, SubToolPassConfig, ToolInvocationStats,
    ToolPassContext, ToolPassRequest, ToolPassResult,
};

use crate::agents::AgentProfile;
use crate::models::{StreamEvent, ToolCall};
use crate::task_board::{
    inject_host_task_board_conversation_id, inject_work_items_tool_host, is_task_board_tool_name,
    maybe_trim_after_tool_pass, task_board_call_is_checkpoint,
};
use crate::tools::normalize_tool_invoke_name;
use crate::tools::parallel::{ParallelLimits, ToolConflictClass};
use crate::tools::parse_tool_call_arguments;
use crate::tools::registry_tool_in_allow_list;
use anyhow::{anyhow, Result};
use batch::{batch_needs_serial_for_approval, plan_tool_batch, PlanToolBatchInput, ToolWave};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;

use super::emit::{emit, emit_task_board_updated, trace_id_opt};
use super::context::TranscriptPersist;
use super::util::{patch_assistant_tool_call_display, tool_display_stream_fields};

use approval::run_approval_gate;
use dispatch::{execute_tool_invocation, invoke_prepared_parallel};
use outcome::record_tool_exec_outcome;
use types::ToolExecResult;

fn task_board_emit_anchor_for_store_key(
    ctx: &ToolPassContext<'_>,
    store_key: &str,
) -> Option<String> {
    if crate::task_board::is_child_store_key(store_key) {
        if let TranscriptPersist::SubLinked(linkage) = &ctx.persist {
            return Some(linkage.trace_id.clone());
        }
    }
    ctx.session
        .state
        .get_main_task_board_anchor(ctx.session.conversation_id, store_key)
        .or_else(|| crate::task_board::anchor_message_id_from_main_turn_key(store_key))
}

struct PreparedTool {
    index: usize,
    tc: ToolCall,
    tool_id: String,
    args_value: serde_json::Value,
    task_board_store_key: String,
}

pub(super) async fn run_agent_tool_pass(mut pass: ToolPassRequest<'_>) -> Result<ToolPassResult> {
    let sub_trace_id = pass.ctx.sub.as_ref().map(|s| s.trace_id.clone());
    let sub_scoped_id = pass
        .ctx
        .sub
        .as_ref()
        .map(|s| s.scoped_message_id.clone());
    let _persist_transcript = pass.ctx.persist_transcript();
    let mut any_executed = false;
    let mut task_board_succeeded = false;
    let mut final_reply_output: Option<String> = None;

    let settings = &pass.ctx.provider.settings;
    let parallel_limits = ParallelLimits::from_settings(settings);
    let force_serial = pass.ctx.lead.as_ref().is_some_and(|l| {
        l.file_tool_lead_for_invoke == AgentProfile::Computer
    }) || !settings.parallel_tool_execution_enabled;

    let mut prepared: Vec<PreparedTool> = Vec::with_capacity(pass.final_tool_calls.len());
    let mut parsed_args: Vec<serde_json::Value> = Vec::new();
    let mut tool_ids: Vec<String> = Vec::new();

    for (index, tc) in pass.final_tool_calls.iter().enumerate() {
        if pass.cancel.is_cancelled() {
            if let Some(consumed) = pass.ctx.consumed_single.as_mut() {
                pass.ctx.tool_budget.sync_out(consumed);
            }
            pass.ctx
                .session
                .state
                .computer_state
                .mark_cancelled(pass.ctx.session.conversation_id);
            return Err(anyhow!("已停止生成"));
        }

        let args_value = parse_tool_call_arguments(&tc.arguments);
        let (tool_id, args_value) = normalize_tool_invoke_name(&tc.name, args_value);
        let mut task_board_store_key = pass.ctx.task_board_store_key.to_string();
        if tool_id == "task_board_init" {
            if let Some(fresh) = crate::task_board::resolve_fresh_main_turn_init_store_key(
                pass.ctx.session.state.task_board_store.as_ref(),
                pass.ctx.session.conversation_id,
                pass.ctx.task_board_store_key,
                pass.ctx.transcript.history,
            ) {
                if let Some(uid) =
                    crate::task_board::latest_real_user_message_id(pass.ctx.transcript.history)
                {
                    pass.ctx.session.state.set_main_task_board_binding(
                        pass.ctx.session.conversation_id,
                        &fresh,
                        &uid,
                    );
                    pass.ctx.session.state.set_active_main_task_board_key(
                        pass.ctx.session.conversation_id,
                        &fresh,
                    );
                    log::info!(
                        "task_board_exec: init fresh board conversation_id={} store_key={} anchor={uid}",
                        pass.ctx.session.conversation_id,
                        fresh
                    );
                }
                task_board_store_key = fresh;
            }
        } else if tool_id == "task_board_abandon"
            && !crate::task_board::is_child_store_key(pass.ctx.task_board_store_key)
        {
            let conv = pass.ctx.session.conversation_id;
            if pass
                .ctx
                .session
                .state
                .get_active_main_task_board_key(conv)
                .as_deref()
                == Some(pass.ctx.task_board_store_key)
            {
                pass.ctx
                    .session
                    .state
                    .clear_active_main_task_board_key(conv);
                log::info!(
                    "task_board_exec: abandoned active board conversation_id={} store_key={}",
                    conv,
                    pass.ctx.task_board_store_key
                );
            }
        }
        let args_value = inject_host_task_board_conversation_id(
            &tool_id,
            args_value,
            pass.ctx.session.conversation_id,
            task_board_store_key.as_str(),
            pass.ctx.transcript.history,
            pass.ctx.task_board_work_items_enabled,
            pass.ctx.task_board_b42_enforced,
        );
        let args_value = inject_work_items_tool_host(
            &tool_id,
            args_value,
            task_board_store_key.as_str(),
            pass.ctx.workspace_root,
            pass.ctx.task_board_work_items_enabled,
        );

        if tool_id.is_empty() {
            emit_tool_failed(
                pass.ctx.session.stream,
                &pass.ctx.message_id,
                tc,
                sub_trace_id.as_deref(),
                sub_scoped_id.as_deref(),
                "工具名为空：请检查 <tool_name>（例如 mouse_click_index、input、response）。",
            );
            super::util::push_tool_result(
                pass.ctx.transcript.history,
                pass.ctx.session.conversation_id,
                &pass.ctx.message_id,
                &tc.id,
                "ERROR: 工具名为空：请检查 <tool_name>（例如 mouse_click_index、input、response）。",
                &pass.ctx.persist,
            );
            any_executed = true;
            continue;
        }

        if pass.ctx.task_board_computer_no_exec_init && tool_id == "task_board_init" {
            let err = "task_board init is handled by the host planner; use task_board_patch or task_board_replace during execution.";
            emit_tool_failed(
                pass.ctx.session.stream,
                &pass.ctx.message_id,
                tc,
                sub_trace_id.as_deref(),
                sub_scoped_id.as_deref(),
                err,
            );
            super::util::push_tool_result(
                pass.ctx.transcript.history,
                pass.ctx.session.conversation_id,
                &pass.ctx.message_id,
                &tc.id,
                &format!("ERROR: {err}"),
                &pass.ctx.persist,
            );
            any_executed = true;
            continue;
        }

        if let Some(sub_cfg) = pass.ctx.sub.as_ref() {
            if !registry_tool_in_allow_list(sub_cfg.allowed_tools, &tool_id) {
                let err = format!("Agent {} 不允许调用工具: {}", sub_cfg.def.id, tc.name);
                emit_tool_failed(
                    pass.ctx.session.stream,
                    &pass.ctx.message_id,
                    tc,
                    sub_trace_id.as_deref(),
                    sub_scoped_id.as_deref(),
                    &err,
                );
                super::util::push_tool_result(
                    pass.ctx.transcript.history,
                    pass.ctx.session.conversation_id,
                    &pass.ctx.message_id,
                    &tc.id,
                    &format!("ERROR: {err}"),
                    &pass.ctx.persist,
                );
                any_executed = true;
                continue;
            }
        }

        tool_ids.push(tool_id.clone());
        parsed_args.push(args_value.clone());
        prepared.push(PreparedTool {
            index,
            tc: tc.clone(),
            tool_id,
            args_value,
            task_board_store_key,
        });
    }

    let approval_serial = batch_needs_serial_for_approval(
        &pass.ctx.session.state.tools,
        pass.ctx.tool_approval_mode,
        &tool_ids,
        &parsed_args,
    );
    let force_serial = force_serial || approval_serial;

    let prep_calls: Vec<ToolCall> = prepared.iter().map(|p| p.tc.clone()).collect();
    let plan = plan_tool_batch(PlanToolBatchInput {
        registry: &pass.ctx.session.state.tools,
        batch: &prep_calls,
        parsed_args: &parsed_args,
        tool_ids: &tool_ids,
        workspace_root: pass.ctx.workspace_root,
        conversation_id: pass.ctx.session.conversation_id,
        force_serial,
        max_parallel_tools: parallel_limits.max_parallel_tools,
    });

    log::info!(
        "tool_batch_exec: mode={:?} waves={} conversation_id={}",
        plan.mode,
        plan.waves.len(),
        pass.ctx.session.conversation_id
    );

    let tool_sem = Arc::new(Semaphore::new(parallel_limits.max_parallel_tools.max(1)));
    let media_sem = Arc::new(Semaphore::new(parallel_limits.max_parallel_media_jobs.max(1)));

    for wave in plan.waves {
        if pass.cancel.is_cancelled() {
            if let Some(consumed) = pass.ctx.consumed_single.as_mut() {
                pass.ctx.tool_budget.sync_out(consumed);
            }
            pass.ctx
                .session
                .state
                .computer_state
                .mark_cancelled(pass.ctx.session.conversation_id);
            return Err(anyhow!("已停止生成"));
        }

        match wave {
            ToolWave::Serial(indices) => {
                for idx in indices {
                    let outcome = run_one_prepared(
                        &mut pass,
                        &prepared,
                        idx,
                        &sub_trace_id,
                        sub_scoped_id.as_deref(),
                    )
                    .await?;
                    apply_one_outcome(
                        &mut pass,
                        &prepared,
                        idx,
                        outcome,
                        &sub_trace_id,
                        &mut any_executed,
                        &mut task_board_succeeded,
                        &mut final_reply_output,
                    )
                    .await;
                }
            }
            ToolWave::Parallel(indices) => {
                let mut wave_outcomes: Vec<(usize, OneToolOutcome)> = Vec::new();
                let mut exec_indices: Vec<usize> = Vec::new();

                for idx in indices {
                    let prep = &prepared[idx];
                    if !run_approval_gate(
                        &mut pass.ctx,
                        &prep.tc,
                        &prep.tool_id,
                        &prep.args_value,
                        sub_trace_id.as_deref(),
                    )
                    .await?
                    {
                        wave_outcomes.push((
                            prep.index,
                            OneToolOutcome {
                                index: prep.index,
                                exec: Ok(("用户已拒绝该工具调用".into(), false, None)),
                                duration_ms: 0,
                                skipped: true,
                            },
                        ));
                        continue;
                    }
                    emit_tool_running(
                        pass.ctx.session.stream,
                        pass.ctx.session.state,
                        pass.ctx.transcript.history,
                        &pass.ctx.message_id,
                        &prep.tc,
                        &prep.tool_id,
                        &prep.args_value,
                        sub_trace_id.as_deref(),
                        sub_scoped_id.as_deref(),
                    );
                    pass.ctx.stats.record_tool_invocation();
                    exec_indices.push(idx);
                }

                let mut exec_futures = Vec::with_capacity(exec_indices.len());
                for idx in exec_indices {
                    let prep = &prepared[idx];
                    let class = pass
                        .ctx
                        .session
                        .state
                        .tools
                        .tool_conflict_class(&prep.tool_id);
                    let tool_sem = tool_sem.clone();
                    let media_sem = media_sem.clone();
                    let stream = pass.ctx.session.stream.clone();
                    let state = pass.ctx.session.state;
                    let provider = pass.ctx.provider;
                    let conversation_id = pass.ctx.session.conversation_id.to_string();
                    let store_key = prep.task_board_store_key.clone();
                    let message_id = pass.ctx.message_id.clone();
                    let tc = prep.tc.clone();
                    let tool_id = prep.tool_id.clone();
                    let args = prep.args_value.clone();
                    let cancel = pass.cancel.clone();
                    let workspace = pass.ctx.workspace_root.to_string();
                    let prep_index = prep.index;
                    let lead_profile = pass
                        .ctx
                        .lead
                        .as_ref()
                        .map(|l| l.file_tool_lead_for_invoke.clone());
                    let sub_profile = pass
                        .ctx
                        .sub
                        .as_ref()
                        .map(|s| s.def.profile.clone());
                    let lead_run_id = pass
                        .ctx
                        .lead
                        .as_ref()
                        .map(|l| l.run_id.to_string());
                    let sub_run_id = pass
                        .ctx
                        .sub
                        .as_ref()
                        .map(|s| s.instance_scope.run_id.clone());

                    exec_futures.push(async move {
                        let _tool_permit = tool_sem.acquire_owned().await;
                        if class == ToolConflictClass::Media {
                            let _media = media_sem.acquire_owned().await;
                        }
                        let started = Instant::now();
                        let exec = invoke_prepared_parallel(
                            &stream,
                            state,
                            provider,
                            &conversation_id,
                            &store_key,
                            &message_id,
                            &tc,
                            &tool_id,
                            args,
                            &workspace,
                            lead_profile,
                            sub_profile,
                            lead_run_id.as_deref(),
                            sub_run_id.as_deref(),
                            &cancel,
                        )
                        .await;
                        OneToolOutcome {
                            index: prep_index,
                            exec,
                            duration_ms: started.elapsed().as_millis() as u64,
                            skipped: false,
                        }
                    });
                }

                let parallel_results = futures_util::future::join_all(exec_futures).await;
                wave_outcomes.extend(
                    parallel_results
                        .into_iter()
                        .map(|o| (o.index, o)),
                );
                wave_outcomes.sort_by_key(|(i, _)| *i);
                for (_, outcome) in wave_outcomes {
                    let idx = outcome.index;
                    let prep_pos = prepared
                        .iter()
                        .position(|p| p.index == idx)
                        .expect("prepared index");
                    apply_one_outcome(
                        &mut pass,
                        &prepared,
                        prep_pos,
                        outcome,
                        &sub_trace_id,
                        &mut any_executed,
                        &mut task_board_succeeded,
                        &mut final_reply_output,
                    )
                    .await;
                }
            }
        }
    }

    if let Some(hook) = pass.trim_hook.as_ref() {
        maybe_trim_after_tool_pass(pass.ctx.transcript.history, hook, task_board_succeeded);
    }

    if pass.ctx.persist.flush_tool_pass_history() {
        crate::conversation_transcript::flush_after_tool_pass(
            pass.ctx.session.conversation_id,
            pass.ctx.transcript.history,
        );
    }

    if !any_executed {
        if let Some(consumed) = pass.ctx.consumed_single.as_mut() {
            pass.ctx.tool_budget.sync_out(consumed);
            return Ok(ToolPassResult::NoopExit);
        }
        if let Some(sub_cfg) = pass.ctx.sub {
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

struct OneToolOutcome {
    index: usize,
    exec: ToolExecResult,
    duration_ms: u64,
    skipped: bool,
}

async fn run_one_prepared(
    pass: &mut ToolPassRequest<'_>,
    prepared: &[PreparedTool],
    prep_index: usize,
    sub_trace_id: &Option<String>,
    sub_scoped_id: Option<&str>,
) -> Result<OneToolOutcome> {
    let prep = &prepared[prep_index];
    let tc = &prep.tc;

    if !run_approval_gate(
        &mut pass.ctx,
        tc,
        &prep.tool_id,
        &prep.args_value,
        sub_trace_id.as_deref(),
    )
    .await?
    {
        return Ok(OneToolOutcome {
            index: prep.index,
            exec: Ok(("用户已拒绝该工具调用".into(), false, None)),
            duration_ms: 0,
            skipped: true,
        });
    }

    emit_tool_running(
        pass.ctx.session.stream,
        pass.ctx.session.state,
        pass.ctx.transcript.history,
        &pass.ctx.message_id,
        tc,
        &prep.tool_id,
        &prep.args_value,
        sub_trace_id.as_deref(),
        sub_scoped_id,
    );
    pass.ctx.stats.record_tool_invocation();
    let started = Instant::now();

    let exec = execute_tool_invocation(
        pass.ctx.session.stream,
        pass.ctx.session.state,
        pass.ctx.provider,
        pass.ctx.session.conversation_id,
        prep.task_board_store_key.as_str(),
        &pass.ctx.message_id,
        pass.ctx.transcript.history,
        tc,
        &prep.tool_id,
        prep.args_value.clone(),
        pass.ctx.lead.as_mut(),
        pass.ctx.sub.as_mut(),
        &pass.cancel,
        pass.ctx.stats,
    )
    .await;

    Ok(OneToolOutcome {
        index: prep.index,
        exec,
        duration_ms: started.elapsed().as_millis() as u64,
        skipped: false,
    })
}

async fn apply_one_outcome(
    pass: &mut ToolPassRequest<'_>,
    prepared: &[PreparedTool],
    prep_index: usize,
    outcome: OneToolOutcome,
    sub_trace_id: &Option<String>,
    any_executed: &mut bool,
    task_board_succeeded: &mut bool,
    final_reply_output: &mut Option<String>,
) {
    if outcome.skipped {
        *any_executed = true;
        return;
    }
    let prep = &prepared[prep_index];
    let tc = &prep.tc;
    let tool_ok = matches!(&outcome.exec, Ok((_, ok, _)) if *ok);
    let final_reply_candidate = outcome.exec.as_ref().ok().and_then(|(out, ok, _)| {
        pass.ctx
            .session
            .state
            .tools
            .should_finalize_after_success(pass.final_tool_calls, &prep.tool_id, *ok)
            .then(|| out.clone())
    });

    record_tool_exec_outcome(
        &mut pass.ctx,
        tc,
        &prep.tool_id,
        &prep.args_value,
        outcome.exec,
        outcome.duration_ms,
        sub_trace_id.as_deref(),
    )
    .await;

    if let Some(out) = final_reply_candidate {
        *final_reply_output = Some(out);
    }
    if tool_ok && is_task_board_tool_name(&prep.tool_id) {
        if task_board_call_is_checkpoint(&prep.tool_id, &prep.args_value) {
            *task_board_succeeded = true;
        }
        let doc = pass
            .ctx
            .session
            .state
            .task_board_store
            .document(prep.task_board_store_key.as_str());
        emit_task_board_updated(
            pass.ctx.session.stream,
            pass.ctx.session.conversation_id,
            prep.task_board_store_key.as_str(),
            task_board_emit_anchor_for_store_key(&pass.ctx, prep.task_board_store_key.as_str()),
            doc.to_value(),
        );
    }
    *any_executed = true;
}

fn emit_tool_failed(
    stream: &super::StreamTx,
    message_id: &str,
    tc: &ToolCall,
    trace_id: Option<&str>,
    scoped_message_id: Option<&str>,
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
            scoped_message_id: trace_id_opt(scoped_message_id),
        },
    );
}

fn emit_tool_running(
    stream: &super::StreamTx,
    state: &super::app_state::AppState,
    history: &mut Vec<crate::models::ChatMessage>,
    message_id: &str,
    tc: &ToolCall,
    tool_id: &str,
    args_value: &serde_json::Value,
    trace_id: Option<&str>,
    scoped_message_id: Option<&str>,
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
            scoped_message_id: trace_id_opt(scoped_message_id),
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
                scoped_message_id: trace_id_opt(scoped_message_id),
            },
        );
    }
}
