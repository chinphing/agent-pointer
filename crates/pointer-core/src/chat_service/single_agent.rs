//! Single-agent streaming loop: prompt hooks per round, then `single_agent_stream` + tools.

use crate::agents::AgentProfile;
use crate::models::{ChatMessage, StreamEvent};
use anyhow::{anyhow, Result};
use std::time::Duration;

use super::computer_pipeline_loop::{
    apply_pipeline_verify_to_tool_card, batch_has_desktop_root_tool, ensure_verify_before_capture,
    pipeline_give_up_error, run_pipeline_post_execute_verify, verify_host_active,
    PipelineLlmUsageRecorder,
};
use super::emit::emit;
use super::util::new_id;

fn latest_round_tool_raw_output(history: &[ChatMessage]) -> Option<String> {
    history
        .iter()
        .rev()
        .find(|m| matches!(m.role, crate::models::Role::Assistant))
        .and_then(|m| {
            if m.status == "streaming" {
                m.tool_raw_output
                    .as_ref()
                    .map(String::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
            } else {
                None
            }
        })
}

pub(super) async fn run_single_agent_loop(
    ctx: &mut super::context::LeadAgentLoopContext<'_>,
) -> Result<()> {
    let stream = ctx.session.stream.clone();
    let state = ctx.session.state.clone();
    let conversation_id = ctx.session.conversation_id;
    let agent_plan = ctx.agent_plan;
    let provider = ctx.provider;
    let settings = ctx.settings;
    let main_task_board_store_key = ctx.main_task_board_store_key;
    let tool_approval_mode = ctx.tool_approval_mode;
    let max_cap = ctx.max_cap;
    let cancel = ctx.session.cancel.clone();
    let reasoning_in_messages = ctx.reasoning_in_messages;
    let lead_profile = state
        .agents
        .get(&agent_plan.lead_agent_id)
        .map(|a| a.def().profile.clone())
        .unwrap_or(AgentProfile::General);
    if lead_profile == AgentProfile::Computer {
        state
            .computer_state
            .reset_for_new_user_guidance(conversation_id);
    }

    const MAX_RETRIES: u32 = 3;
    let mut retry_count: u32 = 0;

    loop {
        match super::agent_round_lifecycle::check_loop_guards(&cancel, ctx.tool_budget) {
            super::agent_round_lifecycle::LoopGuardOutcome::Continue => {}
            super::agent_round_lifecycle::LoopGuardOutcome::Cancelled => {
                ctx.tool_budget.sync_out(ctx.consumed_single);
                return Err(anyhow!("已停止生成"));
            }
            super::agent_round_lifecycle::LoopGuardOutcome::BudgetExhausted => {
                ctx.tool_budget.sync_out(ctx.consumed_single);
                return Err(anyhow!(
                    "本会话单智能体工具调用轮次已达上限（{}）。请新开对话。",
                    max_cap
                ));
            }
        }

        let assistant_id = {
            let id = new_id("msg");
            emit(
                &stream,
                StreamEvent::MessageStart {
                    message_id: id.clone(),
                    conversation_id: conversation_id.to_string(),
                },
            );
            id
        };

        let effective_allowed = agent_plan.allowed_tool_names.clone();
        let tools_system_appendix = crate::tools_system_appendix::generate_tools_system_appendix(
            &state.tools,
            &effective_allowed,
        );
        let tools_appendix_enabled = !tools_system_appendix.is_empty();
        let native_tools = state.tools.openai_tools(&effective_allowed);
        let file_tool_lead_for_invoke = lead_profile.clone();

        let board_store_key = state
            .get_active_main_task_board_key(conversation_id)
            .unwrap_or_else(|| main_task_board_store_key.to_string());

        let mut agent_trace = Vec::new();

        let round_prompts = super::single_agent_prompt::prepare_single_agent_round_prompts(
            super::context::SingleAgentPromptContext {
                session: super::context::SessionRefsArc {
                    stream: &stream,
                    state: state.clone(),
                    conversation_id,
                    cancel: cancel.clone(),
                },
                history: ctx.history,
                agent_plan,
                settings,
                main_task_board_store_key: board_store_key.as_str(),
                assistant_id: &assistant_id,
                lead_profile: lead_profile.clone(),
                tools_system_appendix,
                tools_appendix_enabled,
            },
        )
        .await?;

        let round_settings = if lead_profile == AgentProfile::Computer {
            state
                .computer_state
                .apply_round_settings(conversation_id, settings)
        } else {
            let s = settings.clone();
            if crate::logging::internal_runtime_log_enabled() {
                log::debug!(
                    "llm_round: conversation_id={conversation_id} profile={lead_profile:?} model={}",
                    s.model
                );
            }
            s
        };

        let mut stream_ctx = super::context::LeadStreamRoundContext {
            session: super::context::SessionRefsArc {
                stream: &stream,
                state: state.clone(),
                conversation_id,
                cancel: cancel.clone(),
            },
            provider,
            settings: &round_settings,
            history: ctx.history,
            token_session: ctx.token_session,
            tool_budget: ctx.tool_budget,
            consumed_single: ctx.consumed_single,
            max_cap,
            reasoning_in_messages,
            cancel: cancel.clone(),
        };
        let stream_input = super::context::StreamRoundInput {
            history_for_api: round_prompts.history_for_api,
            system_prompts: round_prompts.system_prompts,
            native_tools,
            tools_appendix_enabled,
        };
        let stream_outcome = super::single_agent_stream::run_provider_stream_round(
            &mut stream_ctx,
            stream_input,
            assistant_id.clone(),
        )
        .await?;
        let buf = match stream_outcome {
            super::single_agent_stream::ProviderRoundOutcome::RetryAfterRecoveryHint => {
                retry_count += 1;
                if retry_count > MAX_RETRIES {
                    log::error!(
                        "recoverable retries exhausted ({}/{MAX_RETRIES}) conversation_id={conversation_id}",
                        retry_count - 1
                    );
                    emit(
                        &stream,
                        StreamEvent::MessageEnd {
                            message_id: assistant_id.clone(),
                            content: None,
                            raw_content: None,
                            tool_raw_output: None,
                            thoughts: None,
                            headline: None,
                            trace_id: None,
                            scoped_message_id: None,
                            attachments: None,
                        },
                    );
                    ctx.tool_budget.sync_out(ctx.consumed_single);
                    state.computer_state.mark_cancelled(conversation_id);
                    return Err(super::emit::chat_run_err(
                        format!(
                            "模型服务连续异常（已重试 {MAX_RETRIES} 次），请稍后重试或检查服务状态。"
                        ),
                        Some(assistant_id.clone()),
                    ));
                }
                let delay = Duration::from_secs(1u64 << (retry_count - 1).min(4));
                tokio::time::sleep(delay).await;
                continue;
            }
            super::single_agent_stream::ProviderRoundOutcome::Completed(b) => b,
        };

        // ── 空响应检测 ──
        let is_empty_response = buf.finish_reason != "length"
            && buf.raw_content_buf.trim().is_empty()
            && buf.final_tool_calls.is_empty();
        if is_empty_response {
            retry_count += 1;
            if retry_count <= MAX_RETRIES {
                log::warn!(
                    "empty response retry {}/{} conversation_id={conversation_id} assistant_id={assistant_id} finish_reason={}",
                    retry_count,
                    MAX_RETRIES,
                    buf.finish_reason
                );
                emit(
                    &stream,
                    StreamEvent::MessageEnd {
                        message_id: assistant_id.clone(),
                        content: None,
                        raw_content: None,
                        tool_raw_output: None,
                        thoughts: None,
                        headline: None,
                        trace_id: None,
                        scoped_message_id: None,
                        attachments: None,
                    },
                );
                let hint = format!(
                    "你的上一次回复为空，既没有文本内容也没有工具调用。请重新处理用户请求，必须给出回复或调用合适的工具。（异常重试 {retry_count}/{MAX_RETRIES}）",
                );
                super::json_tool_retries::push_injected_format_retry_turn(
                    &stream,
                    conversation_id,
                    ctx.history,
                    hint,
                );
                ctx.tool_budget.sync_out(ctx.consumed_single);
                let delay = Duration::from_secs(1u64 << (retry_count - 1).min(4));
                tokio::time::sleep(delay).await;
                continue;
            }
            // 重试耗尽：降级为错误提示
            log::warn!(
                "empty response retries exhausted conversation_id={conversation_id} assistant_id={assistant_id}; falling back to error"
            );
            emit(
                &stream,
                StreamEvent::MessageEnd {
                    message_id: assistant_id.clone(),
                    content: None,
                    raw_content: None,
                    tool_raw_output: None,
                    thoughts: None,
                    headline: None,
                    trace_id: None,
                    scoped_message_id: None,
                    attachments: None,
                },
            );
            ctx.tool_budget.sync_out(ctx.consumed_single);
            return Err(super::emit::chat_run_err(
                "模型连续多次返回空响应，请尝试重新描述问题或新开对话。",
                Some(assistant_id.clone()),
            ));
        }

        // ── 输出截断检测 ──
        if buf.finish_reason == "length" {
            retry_count += 1;
            if retry_count > MAX_RETRIES {
                return Err(super::emit::chat_run_err(
                    format!("模型输出截断重试次数已达上限（{MAX_RETRIES} 次）"),
                    Some(assistant_id.clone()),
                ));
            }
            log::warn!(
                "output truncated conversation_id={conversation_id} assistant_id={assistant_id} finish_reason={}",
                buf.finish_reason
            );
            emit(
                &stream,
                StreamEvent::MessageEnd {
                    message_id: assistant_id.clone(),
                    content: None,
                    raw_content: None,
                    tool_raw_output: None,
                    thoughts: None,
                    headline: None,
                    trace_id: None,
                    scoped_message_id: None,
                    attachments: None,
                },
            );
            let hint = super::json_tool_retries::output_length_retry_supplement(
                crate::models::effective_max_tokens(settings),
                &buf.finish_reason,
            );
            super::json_tool_retries::push_injected_format_retry_turn(
                &stream,
                conversation_id,
                ctx.history,
                hint,
            );
            ctx.tool_budget.sync_out(ctx.consumed_single);
            let delay = Duration::from_secs(1u64 << (retry_count - 1).min(4));
            tokio::time::sleep(delay).await;
            continue;
        }

        // A normal completed round clears transient retry pressure; failures in a later
        // round must not inherit retries from an earlier, already successful round.
        retry_count = 0;

        let lead_scope = ctx.token_session.lead_scope.clone();
        let lead_instance_id = Some(lead_scope.agent_instance_id.clone());
        let mut assistant_msg =
            super::single_agent_post_stream::build_assistant_message_after_stream(
                &assistant_id,
                buf.raw_content_buf.as_str(),
                buf.reasoning_buf,
                reasoning_in_messages,
                &buf.final_tool_calls,
                buf.xml_thoughts,
                agent_plan,
                lead_instance_id.clone(),
                &agent_trace,
                state.as_ref(),
            );
        if assistant_msg.tool_raw_output.is_none() {
            assistant_msg.tool_raw_output = latest_round_tool_raw_output(ctx.history);
        }
        super::single_agent_post_stream::commit_assistant_turn(
            &stream,
            conversation_id,
            ctx.history,
            &assistant_id,
            &assistant_msg,
        );

        if let Err(err) = super::agent_round_lifecycle::computer_round_complete_or_give_up(
            state.as_ref(),
            conversation_id,
            lead_profile.clone(),
            assistant_msg.thoughts.as_deref(),
            assistant_msg.tool_calls.as_deref(),
            false,
        ) {
            ctx.tool_budget.sync_out(ctx.consumed_single);
            return Err(err);
        }

        let budget_scope =
            super::context::ToolBudgetExhaustionScope::lead_single(max_cap, lead_scope.clone());
        let post_action = {
            let mut post_ctx = super::context::PostAssistantContext::new(
                &stream,
                state.as_ref(),
                conversation_id,
                &cancel,
                ctx.history,
                provider,
                settings,
                ctx.tool_budget,
                Some(ctx.consumed_single),
                max_cap,
                &budget_scope,
            );
            super::agent_round_lifecycle::resolve_post_assistant_action(
                &mut post_ctx,
                state.tools.as_ref(),
                &buf.final_tool_calls,
                "lead",
            )
            .await?
        };

        match post_action {
            super::single_agent_post_stream::PostAssistantTurnAction::FinishRun => {
                return Ok(());
            }
            super::single_agent_post_stream::PostAssistantTurnAction::ExecuteTools => {}
        }

        let mut pipeline_before_capture = None;
        if verify_host_active(state.as_ref(), &lead_profile)
            && batch_has_desktop_root_tool(&buf.final_tool_calls, state.tools.as_ref())
        {
            pipeline_before_capture =
                Some(ensure_verify_before_capture(state.as_ref(), conversation_id).await?);
        }

        match super::single_agent_tools::run_single_agent_tool_pass(
            super::agent_tool_pass::LeadSingleToolPassRequest {
                session: super::context::SessionRefs {
                    stream: &stream,
                    state: state.as_ref(),
                    conversation_id,
                    cancel: &cancel,
                },
                main_task_board_store_key: board_store_key.as_str(),
                history: ctx.history,
                allow_agents: &agent_plan.allow_agents,
                enabled_skill_ids: ctx.enabled_skill_ids,
                agent_skill_overrides: ctx.agent_skill_overrides,
                provider,
                tool_approval_mode,
                tool_budget: ctx.tool_budget,
                consumed_single: ctx.consumed_single,
                token_session: ctx.token_session,
                settings,
                lead_agent_id: &agent_plan.lead_agent_id,
                instance_scope: &lead_scope,
                active: super::agent_tool_pass::ActiveAgentExecutionState {
                    def: &agent_plan.active_def,
                    system_prompt: &agent_plan.active_system_prompt,
                    skill_ids: &agent_plan.resolved_skill_ids,
                    skill_prompts: &agent_plan.resolved_skill_prompts,
                    allowed_tools: &agent_plan.allowed_tool_names,
                },
                file_tool_lead_for_invoke: file_tool_lead_for_invoke.clone(),
                assistant_id: assistant_id.clone(),
                final_tool_calls: &buf.final_tool_calls,
                agent_trace: &mut agent_trace,
                cancel: cancel.clone(),
                trigger_source: ctx.trigger_source,
            },
        )
        .await?
        {
            super::single_agent_tools::ToolPassResult::Finished
            | super::single_agent_tools::ToolPassResult::NoopExit => return Ok(()),
            super::single_agent_tools::ToolPassResult::FinalReplyComplete(tool_output) => {
                let delivery_id = new_id("msg");
                emit(
                    &stream,
                    StreamEvent::MessageStart {
                        message_id: delivery_id.clone(),
                        conversation_id: conversation_id.to_string(),
                    },
                );
                let delivery_msg =
                    super::single_agent_post_stream::build_final_reply_delivery_message(
                        &delivery_id,
                        &tool_output,
                        agent_plan,
                        lead_instance_id,
                        state.as_ref(),
                    );
                super::single_agent_post_stream::commit_assistant_turn(
                    &stream,
                    conversation_id,
                    ctx.history,
                    &delivery_id,
                    &delivery_msg,
                );
                ctx.tool_budget.sync_out(ctx.consumed_single);
                return Ok(());
            }
            super::single_agent_tools::ToolPassResult::RanTools => {}
        }

        if verify_host_active(state.as_ref(), &lead_profile)
            && batch_has_desktop_root_tool(&buf.final_tool_calls, state.tools.as_ref())
        {
            let mut pipeline_usage = PipelineLlmUsageRecorder {
                stats: &mut ctx.token_session.stats,
                scope: &ctx.token_session.lead_scope,
            };
            if let Err(err) = run_pipeline_post_execute_verify(
                state.as_ref(),
                &stream,
                conversation_id,
                provider,
                settings,
                pipeline_before_capture.as_ref(),
                &assistant_id,
                ctx.history,
                &buf.final_tool_calls,
                cancel.clone(),
                Some(&mut pipeline_usage),
            )
            .await
            {
                ctx.tool_budget.sync_out(ctx.consumed_single);
                return Err(err);
            }
            let (_last_op, last_verify) = state
                .computer_state
                .pipeline_context_fields(conversation_id);
            if let Some(verify) = last_verify {
                apply_pipeline_verify_to_tool_card(
                    &stream,
                    ctx.history,
                    &assistant_id,
                    &buf.final_tool_calls,
                    state.tools.as_ref(),
                    conversation_id,
                    &verify,
                    true,
                );
            }
            if let Some(err) = pipeline_give_up_error(state.as_ref(), conversation_id) {
                ctx.tool_budget.sync_out(ctx.consumed_single);
                return Err(err);
            }
        }

        {
            let mut post_ctx = super::context::PostAssistantContext::new(
                &stream,
                state.as_ref(),
                conversation_id,
                &cancel,
                ctx.history,
                provider,
                settings,
                ctx.tool_budget,
                Some(ctx.consumed_single),
                max_cap,
                &budget_scope,
            );
            super::agent_round_lifecycle::finish_tool_round_cycle(&mut post_ctx).await?;
        }
    }
}
