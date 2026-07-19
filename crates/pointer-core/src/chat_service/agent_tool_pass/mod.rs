//! Shared tool execution pass after envelope validation (lead single-agent and sub-agent).

mod approval;
mod batch;
mod dispatch;
mod outcome;
mod types;

pub(super) use types::{
    ActiveAgentExecutionState, LeadSingleToolPassRequest, LeadToolPassConfig, SubToolPassConfig,
    ToolInvocationStats, ToolPassContext, ToolPassRequest, ToolPassResult,
};

use crate::agents::AgentProfile;
use crate::models::{StreamEvent, ToolCall};
use crate::task_board::{
    inject_host_task_board_conversation_id, is_task_board_tool_name, maybe_trim_after_tool_pass,
    task_board_call_is_checkpoint,
};
use crate::tools::normalize_tool_invoke_name;
use crate::tools::parallel::{ParallelLimits, ToolConflictClass};
use crate::tools::parse_tool_call_arguments_strict;
use crate::tools::registry_tool_in_allow_list;
use anyhow::{anyhow, Result};
use batch::{batch_needs_serial_for_approval, plan_tool_batch, PlanToolBatchInput, ToolWave};
use futures_util::stream::{FuturesUnordered, StreamExt};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;

use super::context::TranscriptPersist;
use super::emit::{emit, emit_task_board_updated, trace_id_opt};
use super::util::{
    patch_assistant_tool_call_display, patch_assistant_tool_call_outcome,
    tool_display_stream_fields,
};

use approval::run_approval_gate;
use dispatch::{execute_tool_invocation, invoke_prepared_parallel};
use outcome::record_tool_exec_outcome;
use types::ToolExecResult;
use super::run_subagent_delegation::{
    commit_subagent_outcome, execute_self_fork, failed_self_fork_outcome,
    finalize_subagent_outcome, PreparedSubagentOutcome, SelfForkExecutionInput,
    SubagentCommitContext,
};

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

struct SelfForkWaveItem<I> {
    index: usize,
    task_id: String,
    tool_call_id: String,
    input: I,
    /// Deferred `running` notification, published only once the fork actually
    /// acquires a concurrency permit so queued forks are not shown as running.
    running_event: Option<(super::StreamTx, StreamEvent)>,
}

enum SelfForkWaveWork<'a> {
    Execute(SelfForkExecutionInput<'a>),
    Prepared(PreparedSubagentOutcome),
}

async fn collect_self_fork_wave<T, F, Fut>(
    items: Vec<SelfForkWaveItem<T>>,
    semaphore: Arc<Semaphore>,
    limit: usize,
    execute: F,
) -> Vec<(usize, Fut::Output)>
where
    F: Fn(T) -> Fut + Clone,
    Fut: std::future::Future,
{
    let mut futures = FuturesUnordered::new();
    for item in items {
        let semaphore = semaphore.clone();
        let execute = execute.clone();
        futures.push(async move {
            let wait_started = Instant::now();
            let permit = semaphore
                .acquire_owned()
                .await
                .expect("self-fork semaphore must remain open");
            log::info!(
                "run_subagent self-fork permit acquired task_id={} fork_id={} index={} limit={} wait_ms={}",
                item.task_id,
                item.tool_call_id,
                item.index,
                limit,
                wait_started.elapsed().as_millis()
            );
            if let Some((stream, event)) = item.running_event {
                emit(&stream, event);
            }
            let outcome = execute(item.input).await;
            drop(permit);
            (item.index, outcome)
        });
    }

    let mut outcomes = Vec::new();
    while let Some(outcome) = futures.next().await {
        outcomes.push(outcome);
    }
    outcomes.sort_by_key(|(index, _)| *index);
    outcomes
}

pub(super) async fn run_agent_tool_pass(mut pass: ToolPassRequest<'_>) -> Result<ToolPassResult> {
    let sub_trace_id = pass.ctx.sub.as_ref().map(|s| s.trace_id.clone());
    let sub_scoped_id = pass.ctx.sub.as_ref().map(|s| s.scoped_message_id.clone());
    let _persist_transcript = pass.ctx.persist_transcript();
    let mut any_executed = false;
    let mut task_board_succeeded = false;
    let mut final_reply_output: Option<String> = None;

    let settings = &pass.ctx.provider.settings;
    let parallel_limits = ParallelLimits::from_settings(settings);
    let force_serial = pass
        .ctx
        .lead
        .as_ref()
        .is_some_and(|l| l.file_tool_lead_for_invoke == AgentProfile::Computer)
        || !settings.parallel_tool_execution_enabled;

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

        let args_value = match parse_tool_call_arguments_strict(&tc.arguments) {
            Ok(value) => value,
            Err(e) => {
                let err = format!(
                    "工具参数解析失败：{e}. 请检查参数是否为有效 JSON；Windows 路径请使用 / 或转义后的 \\\\。"
                );
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
        };
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
                    pass.ctx
                        .session
                        .state
                        .set_active_main_task_board_key(pass.ctx.session.conversation_id, &fresh);
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
            pass.ctx
                .session
                .state
                .session_index
                .session_user_id(pass.ctx.session.conversation_id)
                .unwrap_or_default()
                .as_str(),
            pass.ctx.transcript.history,
            Some(pass.ctx.session.state),
            pass.ctx.task_board_work_items_enabled,
            false,
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

        if let Some(sub_cfg) = pass.ctx.sub.as_ref() {
            if !registry_tool_in_allow_list(sub_cfg.active.allowed_tools, &tool_id) {
                let err = format!(
                    "Agent {} 不允许调用工具: {}",
                    sub_cfg.active.def.id, tc.name
                );
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
        "tool_batch_exec: mode={:?} waves={} degrade={:?} conversation_id={}",
        plan.mode,
        plan.waves.len(),
        plan.degrade_reason,
        pass.ctx.session.conversation_id
    );

    let tool_sem = Arc::new(Semaphore::new(parallel_limits.max_parallel_tools.max(1)));
    let media_sem = Arc::new(Semaphore::new(
        parallel_limits.max_parallel_media_jobs.max(1),
    ));
    let subagent_sem = Arc::new(Semaphore::new(
        parallel_limits.max_parallel_sub_agents.max(1),
    ));

    for wave in plan.waves {
        match wave {
            ToolWave::Serial(indices) => {
                for idx in indices {
                    if prepared_is_self_fork(&prepared[idx]) {
                        run_self_fork_wave(
                            &mut pass,
                            &prepared,
                            vec![idx],
                            subagent_sem.clone(),
                            parallel_limits.max_parallel_sub_agents,
                            &sub_trace_id,
                            &mut any_executed,
                        )
                        .await?;
                        continue;
                    }
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
                let mut exec_futures: FuturesUnordered<_> = FuturesUnordered::new();
                let mut in_flight_prep: Vec<usize> = Vec::new();
                let mut wave_cancelled = false;

                for idx in indices {
                    if pass.cancel.is_cancelled() {
                        wave_cancelled = true;
                        break;
                    }
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
                        apply_one_outcome(
                            &mut pass,
                            &prepared,
                            idx,
                            OneToolOutcome {
                                index: prep.index,
                                exec: Ok(("用户已拒绝该工具调用".into(), false, None)),
                                duration_ms: 0,
                                skipped: true,
                            },
                            &sub_trace_id,
                            &mut any_executed,
                            &mut task_board_succeeded,
                            &mut final_reply_output,
                        )
                        .await;
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
                        .map(|s| s.active.def.profile.clone());
                    let lead_run_id = pass.ctx.lead.as_ref().map(|l| l.run_id.to_string());
                    let sub_run_id = pass
                        .ctx
                        .sub
                        .as_ref()
                        .map(|s| s.instance_scope.run_id.clone());
                    let agent_instance_id = pass
                        .ctx
                        .sub
                        .as_ref()
                        .map(|s| s.instance_scope.agent_instance_id.clone())
                        .or_else(|| {
                            pass.ctx
                                .lead
                                .as_ref()
                                .map(|l| l.instance_scope.agent_instance_id.clone())
                        });
                    let (web_search_invocation, web_search_history) =
                        if prep.tool_id == "web_search" {
                            let lead_scope = match &*pass.ctx.stats {
                                ToolInvocationStats::TokenSession(session) => {
                                    Some(&session.lead_scope)
                                }
                                ToolInvocationStats::Conversation(_) => None,
                            };
                            let invocation =
                                dispatch::web_search::prepare_web_search_invocation(
                                    lead_scope,
                                    pass.ctx.lead.as_ref().map(|l| l.lead_agent_id),
                                    pass.ctx.sub.as_ref(),
                                )?;
                            (
                                Some(invocation),
                                Some(Arc::new(pass.ctx.transcript.history.clone())),
                            )
                        } else {
                            (None, None)
                        };

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
                            agent_instance_id.as_deref(),
                            web_search_invocation,
                            web_search_history.as_deref().map(|history| history.as_slice()),
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
                    in_flight_prep.push(idx);
                }

                while !exec_futures.is_empty() {
                    tokio::select! {
                        biased;
                        _ = pass.cancel.cancelled() => {
                            wave_cancelled = true;
                            log::info!(
                                "parallel tool wave cancelled conversation_id={}",
                                pass.ctx.session.conversation_id
                            );
                            break;
                        }
                        outcome = exec_futures.next() => {
                            if let Some(outcome) = outcome {
                                let prep_pos = prepared
                                    .iter()
                                    .position(|p| p.index == outcome.index)
                                    .expect("prepared index");
                                in_flight_prep.retain(|&p| p != prep_pos);
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

                if wave_cancelled {
                    for prep_pos in in_flight_prep {
                        emit_tool_pass_cancelled(&mut pass, &prepared[prep_pos], &sub_trace_id);
                    }
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
            }
            ToolWave::ParallelSelfFork(indices) => {
                run_self_fork_wave(
                    &mut pass,
                    &prepared,
                    indices,
                    subagent_sem.clone(),
                    parallel_limits.max_parallel_sub_agents,
                    &sub_trace_id,
                    &mut any_executed,
                )
                .await?;
                // Owned outcomes (including cancelled ones) are already committed above.
                // Short-circuit the pass on cancellation to match the parallel/serial waves
                // so the turn stops instead of issuing another LLM round.
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
                    sub_cfg.active.def,
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

fn prepared_is_self_fork(prepared: &PreparedTool) -> bool {
    prepared.tool_id == "run_subagent"
        && crate::tools::run_subagent::parse_run_subagent_args(&prepared.args_value)
            .is_ok_and(|args| args.is_self_fork())
}

async fn run_self_fork_wave(
    pass: &mut ToolPassRequest<'_>,
    prepared: &[PreparedTool],
    indices: Vec<usize>,
    semaphore: Arc<Semaphore>,
    limit: usize,
    sub_trace_id: &Option<String>,
    any_executed: &mut bool,
) -> Result<()> {
    let mut items = Vec::new();
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
            *any_executed = true;
            continue;
        }

        let max_spawn_depth = pass.ctx.provider.settings.max_sub_agent_spawn_depth.max(1);
        let (active, run_id, parent_spawn_depth) = if let Some(lead) = pass.ctx.lead.as_ref() {
            (lead.active, lead.run_id.to_string(), 0)
        } else if let Some(sub) = pass.ctx.sub.as_ref() {
            (
                sub.active,
                sub.instance_scope.run_id.clone(),
                sub.spawn_depth,
            )
        } else {
            return Err(anyhow!("self-fork execution requires an active parent agent"));
        };
        let running_event = build_self_fork_running_event(
            pass.ctx.session.stream,
            pass.ctx.session.state,
            pass.ctx.transcript.history,
            &pass.ctx.message_id,
            &prep.tc,
            &prep.args_value,
            sub_trace_id.as_deref(),
            pass.ctx
                .sub
                .as_ref()
                .map(|sub| sub.scoped_message_id.as_str()),
        );
        pass.ctx.stats.record_tool_invocation();

        let invocation = dispatch::subagent::prepare_self_fork_invocation(
            pass.ctx.session.state,
            &active,
            &run_id,
            parent_spawn_depth,
            max_spawn_depth,
            pass.ctx.workspace_root,
            &prep.args_value,
            &prep.tc.id,
        );
        let (task_id, work) = match invocation {
            Ok(invocation) => {
                let task_id = invocation.task.id.clone();
                let input = SelfForkExecutionInput {
                    stream: pass.ctx.session.stream,
                    state: pass.ctx.session.state,
                    conversation_id: pass.ctx.session.conversation_id,
                    cancel: pass.cancel.clone(),
                    provider: crate::provider::OpenAIProvider::new(
                        pass.ctx.provider.settings.clone(),
                        pass.ctx.provider.api_key.clone(),
                    ),
                    parent_task_board_store_key: prep.task_board_store_key.clone(),
                    message_id: pass.ctx.message_id.clone(),
                    tool_call_id: prep.tc.id.clone(),
                    run_id: invocation.run_id,
                    task: invocation.task,
                    snapshot: invocation.snapshot,
                    child_spawn_depth: invocation.trace_depth,
                    max_spawn_depth: invocation.max_spawn_depth,
                };
                (task_id, SelfForkWaveWork::Execute(input))
            }
            Err(error) => {
                let task = dispatch::subagent::prepare_self_fork_task(
                    &prep.args_value,
                    &prep.tc.id,
                )
                .unwrap_or_else(|parse_error| crate::agents::AgentTask {
                    id: prep.tc.id.clone(),
                    agent_id: "self".into(),
                    title: "Invalid self fork".into(),
                    goal: "Invalid self-fork invocation".into(),
                    context: parse_error,
                    depends_on: vec![],
                });
                let task_id = task.id.clone();
                let snapshot = dispatch::subagent::build_active_self_fork_snapshot(
                    pass.ctx.session.state,
                    &active,
                    pass.ctx.workspace_root,
                );
                let outcome = failed_self_fork_outcome(
                    &run_id,
                    pass.ctx.session.conversation_id,
                    &prep.tc.id,
                    task,
                    snapshot,
                    parent_spawn_depth.saturating_add(1),
                    error,
                );
                (task_id, SelfForkWaveWork::Prepared(outcome))
            }
        };
        items.push(SelfForkWaveItem {
            index: idx,
            task_id,
            tool_call_id: prep.tc.id.clone(),
            input: work,
            running_event,
        });
    }

    log::info!(
        "run_subagent self-fork wave start conversation_id={} count={} limit={}",
        pass.ctx.session.conversation_id,
        items.len(),
        limit.max(1)
    );
    let outcomes = collect_self_fork_wave(items, semaphore, limit.max(1), |work| async move {
        let started = Instant::now();
        let outcome = match work {
            SelfForkWaveWork::Execute(input) => execute_self_fork(input).await,
            SelfForkWaveWork::Prepared(outcome) => outcome,
        };
        (outcome, started.elapsed().as_millis() as u64)
    })
    .await;

    for (idx, (outcome, duration_ms)) in outcomes {
        apply_self_fork_outcome(pass, &prepared[idx], outcome, duration_ms).await;
        *any_executed = true;
    }
    Ok(())
}

async fn apply_self_fork_outcome(
    pass: &mut ToolPassRequest<'_>,
    prep: &PreparedTool,
    outcome: super::run_subagent_delegation::PreparedSubagentOutcome,
    duration_ms: u64,
) {
    let trace_id = pass.ctx.sub.as_ref().map(|sub| sub.trace_id.clone());
    let pending = {
        let agent_trace = active_parent_trace_mut(&mut pass.ctx.lead, &mut pass.ctx.sub);
        commit_subagent_outcome(
            &mut SubagentCommitContext {
                stream: pass.ctx.session.stream,
                conversation_id: pass.ctx.session.conversation_id,
                message_id: &pass.ctx.message_id,
                history: Some(pass.ctx.transcript.history),
                agent_trace,
                llm_stats: pass.ctx.stats.conversation_stats_mut(),
            },
            outcome,
        )
    };

    let recorded = pending
        .record_tool_result(|exec| async {
            record_tool_exec_outcome(
                &mut pass.ctx,
                &prep.tc,
                &prep.tool_id,
                &prep.args_value,
                exec,
                duration_ms,
                trace_id.as_deref(),
            )
            .await;
        })
        .await;

    let agent_trace = active_parent_trace_mut(&mut pass.ctx.lead, &mut pass.ctx.sub);
    finalize_subagent_outcome(
        &mut SubagentCommitContext {
            stream: pass.ctx.session.stream,
            conversation_id: pass.ctx.session.conversation_id,
            message_id: &pass.ctx.message_id,
            history: Some(pass.ctx.transcript.history),
            agent_trace,
            llm_stats: pass.ctx.stats.conversation_stats_mut(),
        },
        recorded,
    );
}

fn active_parent_trace_mut<'a>(
    lead: &'a mut Option<LeadToolPassConfig<'_>>,
    sub: &'a mut Option<SubToolPassConfig<'_>>,
) -> &'a mut Vec<crate::models::AgentTrace> {
    if let Some(lead) = lead.as_mut() {
        return lead.agent_trace;
    }
    if let Some(sub) = sub.as_mut() {
        return sub.agent_trace;
    }
    unreachable!("self-fork execution requires an active parent agent")
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

fn emit_tool_pass_cancelled(
    pass: &mut ToolPassRequest<'_>,
    prep: &PreparedTool,
    sub_trace_id: &Option<String>,
) {
    let err = "已停止生成";
    let scoped_message_id = pass.ctx.sub.as_ref().map(|s| s.scoped_message_id.as_str());
    emit_tool_failed(
        pass.ctx.session.stream,
        pass.ctx.message_id.as_str(),
        &prep.tc,
        sub_trace_id.as_deref(),
        scoped_message_id,
        err,
    );
    patch_assistant_tool_call_outcome(
        pass.ctx.transcript.history,
        pass.ctx.message_id.as_str(),
        &prep.tc.id,
        "failed",
        None,
        Some(err),
        Some(0),
        None,
        None,
    );
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

/// Patch the assistant tool-call display up front (serial) and build a deferred
/// `running` stream event. The event is emitted only once the self-fork acquires
/// a concurrency permit, so forks queued behind the limit are not shown as running.
fn build_self_fork_running_event(
    stream: &super::StreamTx,
    state: &super::app_state::AppState,
    history: &mut Vec<crate::models::ChatMessage>,
    message_id: &str,
    tc: &ToolCall,
    args_value: &serde_json::Value,
    trace_id: Option<&str>,
    scoped_message_id: Option<&str>,
) -> Option<(super::StreamTx, StreamEvent)> {
    let display = state.tools.format_display(&tc.name, args_value);
    patch_assistant_tool_call_display(history, message_id, &tc.id, &display);
    let (display_label, display_summary) = tool_display_stream_fields(&display);
    let event = StreamEvent::ToolCallStatus {
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
    };
    Some((stream.clone(), event))
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

#[cfg(test)]
mod self_fork_wave_tests {
    use super::batch::{plan_tool_batch, PlanToolBatchInput, ToolWave};
    use super::{collect_self_fork_wave, SelfForkWaveItem};
    use crate::models::ModelSettings;
    use crate::models::ToolCall;
    use crate::tools::parallel::ParallelLimits;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Semaphore;

    fn planned_self_fork_indices(count: usize) -> Vec<usize> {
        let registry = crate::tools::ToolRegistry::new();
        let store = Arc::new(crate::task_board::TaskBoardStore::new());
        crate::tools::builtin::register_all(&registry, store);
        let batch: Vec<_> = (0..count)
            .map(|index| ToolCall {
                id: format!("call-{index}"),
                name: "run_subagent".into(),
                arguments: r#"{"agentId":"self","goal":"work"}"#.into(),
                status: "pending".into(),
                result: None,
                error: None,
                duration_ms: None,
                risk_level: None,
                display_label: None,
                display_summary: None,
            })
            .collect();
        let parsed = vec![serde_json::json!({"agentId": "self", "goal": "work"}); count];
        let tool_ids = vec!["run_subagent".to_string(); count];
        let plan = plan_tool_batch(PlanToolBatchInput {
            registry: &registry,
            batch: &batch,
            parsed_args: &parsed,
            tool_ids: &tool_ids,
            workspace_root: ".",
            conversation_id: "conversation",
            force_serial: false,
            max_parallel_tools: 8,
        });
        match plan.waves.as_slice() {
            [ToolWave::ParallelSelfFork(indices)] => indices.clone(),
            waves => panic!("expected one self-fork wave, got {waves:?}"),
        }
    }

    async fn observed_peak_for_setting(limit: u32) -> usize {
        let mut settings = ModelSettings::default();
        settings.max_parallel_sub_agents = Some(limit);
        let limits = ParallelLimits::from_settings(&settings);
        let semaphore = Arc::new(Semaphore::new(limits.max_parallel_sub_agents));
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));

        let active_for_run = active.clone();
        let peak_for_run = peak.clone();
        let items = (0..3)
            .map(|index| SelfForkWaveItem {
                index,
                task_id: format!("task-{index}"),
                tool_call_id: format!("call-{index}"),
                input: index,
                running_event: None,
            })
            .collect();
        let outcomes = collect_self_fork_wave(
            items,
            semaphore,
            limits.max_parallel_sub_agents,
            move |index| {
                let active = active_for_run.clone();
                let peak = peak_for_run.clone();
                async move {
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    index
                }
            },
        )
        .await;

        assert_eq!(
            outcomes.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        peak.load(Ordering::SeqCst)
    }

    #[tokio::test]
    async fn self_fork_wave_uses_effective_runtime_concurrency_limit() {
        assert_eq!(observed_peak_for_setting(1).await, 1);
        assert_eq!(observed_peak_for_setting(2).await, 2);
    }

    #[tokio::test]
    async fn self_fork_wave_commits_owned_outcomes_in_tool_call_order() {
        let completion_order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let items = (0..2)
            .map(|index| SelfForkWaveItem {
                index,
                task_id: format!("task-{index}"),
                tool_call_id: format!("call-{index}"),
                input: index,
                running_event: None,
            })
            .collect();
        let outcomes = collect_self_fork_wave(
            items,
            Arc::new(Semaphore::new(2)),
            2,
            {
                let completion_order = completion_order.clone();
                move |index| {
                    let completion_order = completion_order.clone();
                    async move {
                        if index == 0 {
                            tokio::time::sleep(Duration::from_millis(30)).await;
                        }
                        completion_order.lock().unwrap().push(index);
                        format!("outcome-{index}")
                    }
                }
            },
        )
        .await;

        assert_eq!(*completion_order.lock().unwrap(), vec![1, 0]);
        let committed = outcomes;
        assert_eq!(
            committed,
            vec![(0, "outcome-0".to_string()), (1, "outcome-1".to_string())]
        );
    }

    #[tokio::test]
    async fn queued_self_fork_starts_when_any_permit_is_released() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let items = planned_self_fork_indices(3)
            .into_iter()
            .map(|prepared_index| SelfForkWaveItem {
                index: prepared_index,
                task_id: format!("task-{prepared_index}"),
                tool_call_id: format!("call-{prepared_index}"),
                input: prepared_index,
                running_event: None,
            })
            .collect();
        collect_self_fork_wave(items, Arc::new(Semaphore::new(2)), 2, {
            let events = events.clone();
            move |index| {
                let events = events.clone();
                async move {
                    events.lock().unwrap().push(format!("start-{index}"));
                    let delay = match index {
                        0 => 10,
                        1 => 80,
                        _ => 0,
                    };
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    events.lock().unwrap().push(format!("finish-{index}"));
                }
            }
        })
        .await;

        let events = events.lock().unwrap();
        let finish_first = events.iter().position(|event| event == "finish-0").unwrap();
        let start_third = events.iter().position(|event| event == "start-2").unwrap();
        let finish_second = events.iter().position(|event| event == "finish-1").unwrap();
        assert!(finish_first < start_third);
        assert!(start_third < finish_second);
    }

    #[tokio::test]
    async fn sibling_failure_keeps_every_self_fork_outcome() {
        let items = (0..3)
            .map(|index| SelfForkWaveItem {
                index,
                task_id: format!("task-{index}"),
                tool_call_id: format!("call-{index}"),
                input: index,
                running_event: None,
            })
            .collect();
        let outcomes = collect_self_fork_wave(
            items,
            Arc::new(Semaphore::new(2)),
            2,
            |index| async move {
                if index == 1 {
                    Err("failed")
                } else {
                    Ok(index)
                }
            },
        )
        .await;

        assert_eq!(outcomes.len(), 3);
        assert!(outcomes[0].1.is_ok());
        assert!(outcomes[1].1.is_err());
        assert!(outcomes[2].1.is_ok());
    }

    #[tokio::test]
    async fn cancellation_keeps_running_and_queued_self_fork_outcomes() {
        let cancel = tokio_util::sync::CancellationToken::new();
        let started = Arc::new(AtomicUsize::new(0));
        let items = (0..3)
            .map(|index| SelfForkWaveItem {
                index,
                task_id: format!("task-{index}"),
                tool_call_id: format!("call-{index}"),
                input: index,
                running_event: None,
            })
            .collect();
        let cancel_when_running = cancel.clone();
        let started_for_cancel = started.clone();
        let cancel_task = tokio::spawn(async move {
            while started_for_cancel.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
            cancel_when_running.cancel();
        });
        let outcomes = collect_self_fork_wave(
            items,
            Arc::new(Semaphore::new(2)),
            2,
            {
                let cancel = cancel.clone();
                let started = started.clone();
                move |_| {
                    let cancel = cancel.clone();
                    let started = started.clone();
                    async move {
                        started.fetch_add(1, Ordering::SeqCst);
                        cancel.cancelled().await;
                        "cancelled"
                    }
                }
            },
        )
        .await;
        cancel_task.await.unwrap();

        assert_eq!(outcomes.len(), 3);
        assert!(outcomes
            .iter()
            .all(|(_, status)| *status == "cancelled"));
    }
}
