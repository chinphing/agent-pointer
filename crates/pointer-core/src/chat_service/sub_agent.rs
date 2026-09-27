//! Sub-agent (`run_sub_agent`) tool loop: isolated history, shared stream/post-stream/tool-pass with lead.

use anyhow::{anyhow, Result};
use std::time::Duration;

use crate::agents::{agent_display_label, AgentProfile, AgentRunResult};
use crate::models::{effective_reasoning_in_messages, ChatMessage, Role, StreamEvent};

use super::agent_post_stream::{
    build_sub_assistant_message_after_stream, commit_sub_assistant_turn, sub_agent_run_result,
    PostAssistantTurnAction,
};
use super::agent_round_lifecycle;
use super::agent_tool_pass::{
    run_agent_tool_pass, SubToolPassConfig, ToolInvocationStats, ToolPassResult,
};
use super::computer_pipeline_loop::{
    apply_pipeline_verify_to_tool_card, batch_has_desktop_root_tool, ensure_verify_before_capture,
    pipeline_give_up_error, run_pipeline_post_execute_verify, verify_host_active,
    PipelineLlmUsageRecorder,
};
use super::context::{PostAssistantContext, ToolBudgetExhaustionScope, TranscriptPersist};
use super::emit::{emit, trace_id_opt};
use super::session_model::sub_agent_provider;
use super::sub_agent_prompt::{
    init_sub_agent_session, prepare_sub_agent_round_prompts, SubAgentDefinitionSource,
};
use super::sub_agent_stream::{run_sub_agent_stream_round, SubAgentStreamOutcome};
use super::sub_message::SubMessageLinkage;
use super::util::new_id;
use crate::task_board::TaskBoardTrimHook;

fn emit_retry_span(
    trace_bus: &crate::observability::TraceBus,
    run_id: &str,
    conversation_id: &str,
    reason: &str,
    attempt: u32,
    delay_ms: u64,
) {
    let mut span = crate::observability::TraceEvent::new(
        run_id,
        uuid::Uuid::new_v4().to_string(),
        crate::observability::SpanKind::Retry,
        reason,
    );
    span.parent_span_id = Some("run-root".to_string());
    span.run_id = run_id.to_string();
    span.conversation_id = conversation_id.to_string();
    span.attributes = serde_json::json!({ "attempt": attempt, "delay_ms": delay_ms });
    span.end();
    trace_bus.emit(span);
}

/// Final handoff for `run_subagent`: prefer the latest assistant turn (final Markdown digest),
/// fall back to accumulated stream content when that turn is empty.
fn sub_agent_handoff_content(local_history: &[ChatMessage], accumulated: &str) -> String {
    local_history
        .iter()
        .rev()
        .find(|m| matches!(m.role, Role::Assistant))
        .map(|m| m.content.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| accumulated.trim().to_string())
}

fn sub_agent_execution_provider(
    parent: &crate::provider::OpenAIProvider,
    definition_source: &SubAgentDefinitionSource<'_>,
) -> crate::provider::OpenAIProvider {
    match definition_source {
        SubAgentDefinitionSource::Registered(task) => sub_agent_provider(parent, &task.agent_id),
        SubAgentDefinitionSource::Snapshot(snapshot) => {
            let mut settings = parent.settings.clone();
            settings.workspace_root = snapshot.workspace_root.trim().to_string();
            crate::provider::OpenAIProvider::new(settings, parent.api_key.clone())
        }
    }
}

pub(crate) async fn run_sub_agent(
    ctx: &mut super::context::SubAgentLoopContext<'_>,
) -> Result<AgentRunResult> {
    let provider = ctx.provider;
    let state = ctx.session.state;
    let stream = ctx.session.stream;
    let conversation_id = ctx.session.conversation_id;
    let trace_bus = state.trace_bus.clone();
    let parent_task_board_store_key = ctx.parent_task_board_store_key;
    let message_id = ctx.message_id;
    let task = ctx.task;
    let cancel = ctx.session.cancel.clone();
    let sub_provider = sub_agent_execution_provider(provider, &ctx.definition_source);
    let reasoning_in_messages = effective_reasoning_in_messages(&sub_provider.settings);
    let session = init_sub_agent_session(
        state,
        &sub_provider,
        conversation_id,
        message_id,
        parent_task_board_store_key,
        task,
        &ctx.definition_source,
        &ctx.instance_scope,
        ctx.enabled_skill_ids,
        ctx.agent_skill_overrides,
        ctx.spawn_depth,
        ctx.max_spawn_depth,
        ctx.resume_history.take(),
    )?;
    let def = session.def;
    let system_prompt = session.system_prompt;
    let skill_ids = session.skill_ids;
    let skill_prompts = session.skill_prompts;
    let instance_scope = session.instance_scope;
    let sub_run_id = instance_scope.run_id.clone();
    let trace_id = session.trace_id;
    let session_extras = session.session_extras;
    let task_dynamic_blocks = session.task_dynamic_blocks;
    let tools_system_appendix = session.tools_system_appendix;
    let allowed_tools = session.allowed_tools;
    let allow_agents = session.allow_agents;
    let sub_task_board_key = session.sub_task_board_key;
    let tool_approval_mode = session.tool_approval_mode;
    let mut local_history = session.local_history;
    let spawn_depth = session.spawn_depth;
    let compress_lease = crate::context_compression::SubAgentPrecompressLease::new(
        crate::context_compression::sub_agent_compression_queue_key(
            conversation_id,
            &instance_scope.agent_instance_id,
        ),
    );
    let sub_linkage = SubMessageLinkage {
        anchor_message_id: message_id.to_string(),
        trace_id: trace_id.clone(),
        task_id: task.id.clone(),
        spawn_depth,
        agent_instance_id: instance_scope.agent_instance_id.clone(),
    };
    let max_cap = ctx.sub_tool_budget.cap();
    let tools_appendix_enabled = !tools_system_appendix.is_empty();
    let native_tools = state.tools.openai_tools(&allowed_tools);
    let budget_scope = ToolBudgetExhaustionScope::sub_agent(max_cap, instance_scope.clone());
    let mut content = String::new();

    if def.profile == AgentProfile::Computer {
        state
            .computer_state
            .reset_for_new_user_guidance(conversation_id);
    }

    const MAX_RETRIES: u32 = 3;
    let mut retry_count: u32 = 0;
    let mut overflow_recoveries: u32 = 0;

    // Set thread-local for this sub-agent's tool calls; restore parent on exit.
    let _agent_guard =
        crate::tools::file::AgentWorkspaceGuard::enter(&sub_provider.settings.workspace_root);

    let is_computer = def.profile == AgentProfile::Computer;
    let work_items_enabled = is_computer;

    loop {
        match agent_round_lifecycle::check_loop_guards(&cancel, ctx.sub_tool_budget) {
            agent_round_lifecycle::LoopGuardOutcome::Continue => {}
            agent_round_lifecycle::LoopGuardOutcome::Cancelled => {
                state.computer_state.mark_cancelled(conversation_id);
                let loc = crate::i18n::current_ui_locale();
                let toast_msg = if def.profile == AgentProfile::Computer {
                    crate::i18n::t("toast.subagent_cancelled_computer", loc)
                } else {
                    crate::i18n::t("toast.subagent_cancelled_generic", loc)
                };
                crate::stream_broadcast::publish_stream(
                    &stream,
                    StreamEvent::UiToast {
                        conversation_id: conversation_id.to_string(),
                        message: toast_msg.to_string(),
                        level: "warning".to_string(),
                    },
                );
                return Err(anyhow!("已停止生成"));
            }
            agent_round_lifecycle::LoopGuardOutcome::BudgetExhausted => {
                state.computer_state.mark_cancelled(conversation_id);
                return Err(anyhow!(
                    "子 Agent 工具调用轮次已达上限（{}）。请新开对话或在设置中调高上限。",
                    max_cap
                ));
            }
        }

        crate::context_compression::prepare_sub_agent_history_between_llm_rounds(
            &mut local_history,
            &sub_provider.settings,
            &sub_provider,
            conversation_id,
            stream,
            cancel.clone(),
            crate::context_compression::CompressionUiContext::sub_agent(
                instance_scope.clone(),
                message_id,
                &def.id,
                &agent_display_label(&def),
                &task.id,
            ),
            ctx.llm_stats.last_round_prompt_tokens,
            &compress_lease,
        )
        .await;
        if cancel.is_cancelled() {
            state.computer_state.mark_cancelled(conversation_id);
            return Err(anyhow!("已停止生成"));
        }

        let round_message_id = new_id("agent_msg");
        let round_placeholder = ChatMessage {
            id: round_message_id.clone(),
            role: Role::Assistant,
            content: String::new(),
            status: "streaming".into(),
            created_at: super::util::now_ms(),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
            error_message: None,
            reasoning: None,
            thoughts: None,
            headline: None,
            raw_content: None,
            tool_raw_output: None,
            agent_id: Some(def.id.clone()),
            agent_instance_id: Some(instance_scope.agent_instance_id.clone()),
            agent_name: None,
            agent_trace: None,
            image_slot_labels: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
            attachments: None,
            anchor_message_id: None,
            trace_id: None,
            task_id: None,
            spawn_depth: None,
        };
        super::sub_message::persist_sub_message(conversation_id, &sub_linkage, &round_placeholder);
        emit(
            stream,
            StreamEvent::SubMessageStart {
                conversation_id: conversation_id.to_string(),
                anchor_message_id: sub_linkage.anchor_message_id.clone(),
                scoped_message_id: round_message_id.clone(),
                trace_id: sub_linkage.trace_id.clone(),
                task_id: sub_linkage.task_id.clone(),
                spawn_depth: sub_linkage.spawn_depth,
                agent_instance_id: sub_linkage.agent_instance_id.clone(),
            },
        );

        let round_prompts =
            prepare_sub_agent_round_prompts(super::context::SubAgentPromptContext {
                session: super::context::SessionRefs {
                    stream,
                    state,
                    conversation_id,
                    cancel: &cancel,
                },
                message_id,
                task_id: &task.id,
                round_message_id: &round_message_id,
                local_history: &local_history,
                session_extras: &session_extras,
                task_dynamic_blocks: &task_dynamic_blocks,
                tools_system_appendix: &tools_system_appendix,
                sub_task_board_key: &sub_task_board_key,
                def: &def,
                workspace_root: sub_provider.settings.workspace_root.as_str(),
                user_dynamic_inject_enabled: sub_provider.settings.user_dynamic_inject_enabled,
                spawn_depth,
            })
            .await?;

        let mut stream_ctx = super::context::SubStreamRoundContext {
            session: super::context::SessionRefs {
                stream,
                state,
                conversation_id,
                cancel: &cancel,
            },
            provider: &sub_provider,
            sub_tool_budget: ctx.sub_tool_budget,
            max_cap,
            reasoning_in_messages,
            cancel: cancel.clone(),
            overflow_recoveries,
        };
        let mut stream_refs = super::context::SubStreamRoundRefs {
            task,
            def: &def,
            instance_scope: &instance_scope,
            trace_id: &trace_id,
            message_id,
            round_message_id: &round_message_id,
            session_content: &mut content,
            local_history: &mut local_history,
            llm_stats: ctx.llm_stats,
        };
        let stream_input = super::context::StreamRoundInput {
            injected_tail: round_prompts.injected_tail,
            system_prompts: round_prompts.system_prompts,
            native_tools: native_tools.clone(),
            tools_appendix_enabled,
        };
        let stream_outcome =
            run_sub_agent_stream_round(&mut stream_ctx, &mut stream_refs, stream_input).await?;

        let buf = match stream_outcome {
            SubAgentStreamOutcome::RetryAfterOverflowCompress => {
                overflow_recoveries += 1;
                log::info!(
                    "sub_agent: overflow recovery {}/{} task_id={} agent={}",
                    overflow_recoveries,
                    crate::context_compression::MAX_OVERFLOW_RECOVERIES,
                    task.id,
                    def.id
                );
                continue;
            }
            SubAgentStreamOutcome::RetryAfterRecoveryHint => {
                retry_count += 1;
                if retry_count > MAX_RETRIES {
                    log::error!(
                        "recoverable retries exhausted ({}/{MAX_RETRIES}) sub_agent task_id={} agent={}",
                        retry_count - 1,
                        task.id,
                        def.id
                    );
                    state.computer_state.mark_cancelled(conversation_id);
                    return Err(super::emit::chat_run_err(
                        format!(
                            "模型服务连续异常（已重试 {MAX_RETRIES} 次），请稍后重试或检查服务状态。"
                        ),
                        Some(message_id.to_string()),
                    ));
                }
                let delay = Duration::from_secs(1u64 << (retry_count - 1).min(4));
                emit_retry_span(
                    &trace_bus,
                    &sub_run_id,
                    conversation_id,
                    "recoverable",
                    retry_count,
                    delay.as_millis() as u64,
                );
                tokio::time::sleep(delay).await;
                continue;
            }
            SubAgentStreamOutcome::Completed(b) => {
                if overflow_recoveries > 0 {
                    log::info!(
                        "sub_agent: overflow recovery streak reset after successful round task_id={} agent={} had_recoveries={}",
                        task.id,
                        def.id,
                        overflow_recoveries
                    );
                    overflow_recoveries = 0;
                }
                b
            }
        };

        // 截断优先于空响应：length 可能没有可见 content，但并非真正的空响应。
        if buf.finish_reason == "length" {
            retry_count += 1;
            if retry_count > MAX_RETRIES {
                return Err(anyhow!(
                    "子 Agent 输出截断重试次数已达上限（{MAX_RETRIES} 次）"
                ));
            }
            log::warn!(
                "output truncated sub_agent task_id={} agent={} retry={}/{}",
                task.id,
                def.id,
                retry_count,
                MAX_RETRIES
            );
            emit(
                stream,
                StreamEvent::MessageEnd {
                    message_id: message_id.to_string(),
                    content: None,
                    raw_content: None,
                    tool_raw_output: None,
                    thoughts: None,
                    headline: None,
                    trace_id: trace_id_opt(Some(&sub_linkage.trace_id)),
                    scoped_message_id: trace_id_opt(Some(&round_message_id)),
                    attachments: None,
                },
            );
            let hint = super::json_tool_retries::output_length_retry_supplement(
                crate::models::effective_max_tokens(&sub_provider.settings),
                &buf.finish_reason,
            );
            super::json_tool_retries::push_injected_format_retry_turn(
                stream,
                conversation_id,
                &mut local_history,
                hint,
            );
            let delay = Duration::from_secs(1u64 << (retry_count - 1).min(4));
            emit_retry_span(
                &trace_bus,
                &sub_run_id,
                conversation_id,
                "length",
                retry_count,
                delay.as_millis() as u64,
            );
            tokio::time::sleep(delay).await;
            continue;
        }

        if buf.raw_content_buf.trim().is_empty() && buf.final_tool_calls.is_empty() {
            retry_count += 1;
            if retry_count > MAX_RETRIES {
                return Err(anyhow!(
                    "子 Agent 连续返回空响应，重试次数已达上限（{MAX_RETRIES} 次）"
                ));
            }
            log::warn!(
                "empty response retry {}/{} sub_agent task_id={} agent={} finish_reason={}",
                retry_count,
                MAX_RETRIES,
                task.id,
                def.id,
                buf.finish_reason
            );
            emit(
                stream,
                StreamEvent::MessageEnd {
                    message_id: message_id.to_string(),
                    content: None,
                    raw_content: None,
                    tool_raw_output: None,
                    thoughts: None,
                    headline: None,
                    trace_id: trace_id_opt(Some(&sub_linkage.trace_id)),
                    scoped_message_id: trace_id_opt(Some(&round_message_id)),
                    attachments: None,
                },
            );
            let hint = format!(
                "你的上一次回复为空，既没有文本内容也没有工具调用。请重新处理当前子任务，必须给出回复或调用合适的工具。（异常重试 {retry_count}/{MAX_RETRIES}）",
            );
            super::json_tool_retries::push_injected_format_retry_turn(
                stream,
                conversation_id,
                &mut local_history,
                hint,
            );
            let delay = Duration::from_secs(1u64 << (retry_count - 1).min(4));
            emit_retry_span(
                &trace_bus,
                &sub_run_id,
                conversation_id,
                "empty",
                retry_count,
                delay.as_millis() as u64,
            );
            tokio::time::sleep(delay).await;
            continue;
        }

        // A normal completed round clears transient retry pressure.
        retry_count = 0;

        // Align with lead-agent rounds: mark sub session stream ended so thoughts collapse between sub rounds.
        emit(
            stream,
            StreamEvent::MessageEnd {
                message_id: message_id.to_string(),
                content: None,
                raw_content: None,
                tool_raw_output: None,
                thoughts: None,
                headline: None,
                trace_id: trace_id_opt(Some(&sub_linkage.trace_id)),
                scoped_message_id: trace_id_opt(Some(&round_message_id)),
                attachments: None,
            },
        );

        let assistant_msg = build_sub_assistant_message_after_stream(
            &round_message_id,
            buf.raw_content_buf,
            buf.reasoning_buf,
            reasoning_in_messages,
            &buf.final_tool_calls,
            buf.xml_thoughts,
            &def,
            Some(instance_scope.agent_instance_id.clone()),
            state,
        );
        commit_sub_assistant_turn(
            stream,
            conversation_id,
            &mut local_history,
            assistant_msg,
            &sub_linkage,
        );

        let last_msg = local_history.last();
        if let Err(err) = agent_round_lifecycle::computer_round_complete_or_give_up(
            state,
            conversation_id,
            def.profile.clone(),
            last_msg.and_then(|m| m.thoughts.as_deref()),
            last_msg.and_then(|m| m.tool_calls.as_deref()),
            true,
        ) {
            return Err(err);
        }

        let post_action = {
            let mut post_ctx = PostAssistantContext::new(
                stream,
                state,
                conversation_id,
                &cancel,
                &mut local_history,
                &sub_provider,
                &sub_provider.settings,
                ctx.sub_tool_budget,
                None,
                max_cap,
                &budget_scope,
            );
            agent_round_lifecycle::resolve_post_assistant_action(
                &mut post_ctx,
                state.tools.as_ref(),
                &buf.final_tool_calls,
                "sub-agent",
            )
            .await?
        };

        match post_action {
            PostAssistantTurnAction::FinishRun => {
                if crate::task_board::maybe_auto_finalize_if_complete(
                    &state.task_board_store,
                    &sub_task_board_key,
                ) {
                    let doc = state.task_board_store.document(&sub_task_board_key);
                    super::emit::emit_task_board_updated(
                        &stream,
                        conversation_id,
                        &sub_task_board_key,
                        Some(sub_linkage.trace_id.clone()),
                        doc.to_value(),
                    );
                }
                return Ok(sub_agent_run_result(
                    &task.id,
                    &def,
                    sub_agent_handoff_content(&local_history, &content),
                    &instance_scope.agent_instance_id,
                ));
            }
            PostAssistantTurnAction::ExecuteTools => {}
        }

        let mut pipeline_before_capture = None;
        if verify_host_active(state, &def.profile)
            && batch_has_desktop_root_tool(&buf.final_tool_calls, state.tools.as_ref())
        {
            pipeline_before_capture =
                Some(ensure_verify_before_capture(state, conversation_id).await?);
        }

        let sub_cfg = SubToolPassConfig {
            task,
            allow_agents: &allow_agents,
            agent_skill_overrides: ctx.agent_skill_overrides,
            instance_scope: &instance_scope,
            agent_trace: ctx.agent_trace,
            accumulated_content: content.clone(),
            trace_id: sub_linkage.trace_id.clone(),
            spawn_depth,
            scoped_message_id: round_message_id.clone(),
            active: super::agent_tool_pass::ActiveAgentExecutionState {
                def: &def,
                system_prompt: &system_prompt,
                skill_ids: &skill_ids,
                skill_prompts: &skill_prompts,
                allowed_tools: &allowed_tools,
            },
            background_job_id: ctx.background_job_id.as_deref(),
        };
        let mut stats = ToolInvocationStats::Conversation(ctx.llm_stats);
        let anchor_message_id =
            state.get_main_task_board_anchor(conversation_id, &sub_task_board_key);
        let trim_hook = TaskBoardTrimHook {
            settings: &sub_provider.settings,
            agent_id: &def.id,
            conversation_id,
            stream,
            emit_trim_ui_event: false,
            anchor_message_id: anchor_message_id.as_deref(),
        };
        let pass = super::agent_tool_pass::ToolPassRequest {
            ctx: super::agent_tool_pass::ToolPassContext {
                session: super::context::SessionRefs {
                    stream,
                    state,
                    conversation_id,
                    cancel: &cancel,
                },
                transcript: super::context::TranscriptRefs {
                    history: &mut local_history,
                },
                persist: TranscriptPersist::SubLinked(sub_linkage.clone()),
                message_id: round_message_id.clone(),
                task_board_store_key: &sub_task_board_key,
                tool_approval_mode: &tool_approval_mode,
                tool_budget: ctx.sub_tool_budget,
                consumed_single: None,
                provider: &sub_provider,
                stats: &mut stats,
                lead: None,
                sub: Some(sub_cfg),
                task_board_work_items_enabled: work_items_enabled,
                workspace_root: &sub_provider.settings.workspace_root,
                trigger_source: None,
                ask_user_deferred: std::sync::atomic::AtomicBool::new(false),
                state_arc: ctx.state_arc.clone(),
            },
            final_tool_calls: &buf.final_tool_calls,
            trim_hook: Some(trim_hook),
            cancel: cancel.clone(),
        };
        match Box::pin(run_agent_tool_pass(pass)).await? {
            ToolPassResult::SubFinished(result) => return Ok(result),
            ToolPassResult::FinalReplyComplete(output) => {
                return Ok(sub_agent_run_result(
                    &task.id,
                    &def,
                    output,
                    &instance_scope.agent_instance_id,
                ));
            }
            ToolPassResult::NoopExit => {
                return Ok(sub_agent_run_result(
                    &task.id,
                    &def,
                    sub_agent_handoff_content(&local_history, &content),
                    &instance_scope.agent_instance_id,
                ));
            }
            ToolPassResult::RanTools => {}
            ToolPassResult::AskUserDeferred => {}
        }

        if verify_host_active(state, &def.profile)
            && batch_has_desktop_root_tool(&buf.final_tool_calls, state.tools.as_ref())
        {
            let mut pipeline_usage = PipelineLlmUsageRecorder {
                stats: ctx.llm_stats,
                scope: &instance_scope,
                source: crate::llm_token_stats::active_provider_source(&sub_provider.settings),
            };
            run_pipeline_post_execute_verify(
                state,
                stream,
                conversation_id,
                &sub_provider,
                &sub_provider.settings,
                pipeline_before_capture.as_ref(),
                &round_message_id,
                &local_history,
                &buf.final_tool_calls,
                cancel.clone(),
                Some(&mut pipeline_usage),
            )
            .await?;
            let (_last_op, last_verify) = state
                .computer_state
                .pipeline_context_fields(conversation_id);
            if let Some(verify) = last_verify {
                apply_pipeline_verify_to_tool_card(
                    stream,
                    &mut local_history,
                    &round_message_id,
                    &buf.final_tool_calls,
                    state.tools.as_ref(),
                    conversation_id,
                    &verify,
                    false,
                );
            }
            if let Some(err) = pipeline_give_up_error(state, conversation_id) {
                return Err(err);
            }
        }

        {
            let mut post_ctx = PostAssistantContext::new(
                stream,
                state,
                conversation_id,
                &cancel,
                &mut local_history,
                &sub_provider,
                &sub_provider.settings,
                ctx.sub_tool_budget,
                None,
                max_cap,
                &budget_scope,
            );
            agent_round_lifecycle::finish_tool_round_cycle(&mut post_ctx).await?;
        }
    }
}

#[cfg(test)]
mod handoff_tests {
    use super::*;
    use crate::models::ChatMessage;

    fn assistant(content: &str) -> ChatMessage {
        ChatMessage {
            id: "a".into(),
            role: Role::Assistant,
            content: content.into(),
            status: "done".into(),
            created_at: 0,
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
            error_message: None,
            reasoning: None,
            thoughts: None,
            headline: None,
            raw_content: None,
            tool_raw_output: None,
            agent_id: None,
            agent_instance_id: None,
            agent_name: None,
            agent_trace: None,
            image_slot_labels: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
            attachments: None,
            anchor_message_id: None,
            trace_id: None,
            task_id: None,
            spawn_depth: None,
        }
    }

    #[test]
    fn handoff_prefers_latest_assistant_turn() {
        let history = vec![assistant(""), assistant("## Summary\nDone.")];
        assert_eq!(
            sub_agent_handoff_content(&history, "stale accumulated"),
            "## Summary\nDone."
        );
    }

    #[test]
    fn handoff_falls_back_to_accumulated_when_last_assistant_empty() {
        let history = vec![assistant("")];
        assert_eq!(
            sub_agent_handoff_content(&history, "  fallback  "),
            "fallback"
        );
    }
}

#[cfg(test)]
mod execution_provider_tests {
    use super::sub_agent_execution_provider;
    use crate::agents::{
        AccessPolicy, AgentDef, AgentProfile, AgentTask, AgentUiConfig, SkillsPolicy,
    };
    use crate::chat_service::self_fork::SelfForkSnapshot;
    use crate::chat_service::sub_agent_prompt::SubAgentDefinitionSource;
    use crate::models::{AgentModelRef, ModelSettings};
    use crate::provider::OpenAIProvider;
    use std::collections::HashMap;

    fn snapshot() -> SelfForkSnapshot {
        SelfForkSnapshot {
            def: AgentDef {
                id: "current-agent".into(),
                name: "Current Agent".into(),
                description: "snapshot".into(),
                role: "worker".into(),
                profile: AgentProfile::Coder,
                default_skill_ids: vec![],
                skills_policy: SkillsPolicy::InheritsFromParent,
                access_policy: AccessPolicy::default(),
                builtin: false,
                enabled: true,
                tool_names: vec![],
                source: None,
                resource_files: vec![],
                allow_agents: vec![],
                config: HashMap::new(),
                ui: AgentUiConfig::default(),
                plugin_id: None,
            },
            system_prompt: "current prompt".into(),
            skill_ids: vec![],
            skill_prompts: vec![],
            allowed_tools: vec![],
            workspace_root: "/snapshot/workspace".into(),
        }
    }

    fn parent_provider() -> OpenAIProvider {
        let settings = ModelSettings {
            active_provider_id: "current-provider".into(),
            model: "current-model".into(),
            workspace_root: "/parent/workspace".into(),
            agent_default_models: [
                (
                    "self".into(),
                    AgentModelRef {
                        provider_id: "wrong-provider".into(),
                        model: "wrong-self-model".into(),
                    },
                ),
                (
                    "explore".into(),
                    AgentModelRef {
                        provider_id: "registered-provider".into(),
                        model: "registered-model".into(),
                    },
                ),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        OpenAIProvider::new(settings, "current-key".into())
    }

    #[test]
    fn snapshot_uses_current_provider_and_snapshot_workspace_without_self_override() {
        let snapshot = snapshot();
        let source = SubAgentDefinitionSource::Snapshot(&snapshot);

        let provider = sub_agent_execution_provider(&parent_provider(), &source);

        assert_eq!(provider.settings.active_provider_id, "current-provider");
        assert_eq!(provider.settings.model, "current-model");
        assert_eq!(provider.settings.workspace_root, "/snapshot/workspace");
        assert_eq!(provider.api_key, "current-key");
    }

    #[test]
    fn registered_source_keeps_agent_model_override_and_parent_workspace() {
        let task = AgentTask {
            id: "task".into(),
            agent_id: "explore".into(),
            title: "Explore".into(),
            goal: "Inspect".into(),
            context: String::new(),
            depends_on: vec![],
        };
        let source = SubAgentDefinitionSource::Registered(&task);

        let provider = sub_agent_execution_provider(&parent_provider(), &source);

        assert_eq!(provider.settings.active_provider_id, "registered-provider");
        assert_eq!(provider.settings.model, "registered-model");
        assert_eq!(provider.settings.workspace_root, "/parent/workspace");
    }
}
