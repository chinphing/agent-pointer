//! Sub-agent (`run_sub_agent`) tool loop: isolated history, shared stream/post-stream/tool-pass with lead.

use anyhow::{anyhow, Result};

use crate::agent_instance_scope::AgentInstanceScope;
use crate::agents::{AgentProfile, AgentRunResult};
use crate::models::{effective_reasoning_in_messages, ChatMessage, Role, StreamEvent};

use super::agent_post_stream::{
    build_sub_assistant_message_after_stream, commit_sub_assistant_turn, sub_agent_run_result,
    PostAssistantTurnAction,
};
use super::context::{PostAssistantContext, ToolBudgetExhaustionScope, TranscriptPersist};
use super::agent_round_lifecycle;
use super::agent_tool_pass::{
    run_agent_tool_pass, SubToolPassConfig, ToolInvocationStats, ToolPassResult,
};
use super::computer_pipeline_loop::{
    apply_pipeline_verify_to_tool_card, batch_has_desktop_root_tool, ensure_verify_before_capture,
    pipeline_give_up_error, run_pipeline_post_execute_verify, verify_host_active,
    PipelineLlmUsageRecorder,
};
use crate::task_board::TaskBoardTrimHook;
use crate::task_board::planner::PlannerRunOutcome;
use super::emit::{agent_trace_step_id, emit, trace_id_opt};
use super::session_model::sub_agent_provider;
use super::sub_agent_prompt::{init_sub_agent_session, prepare_sub_agent_round_prompts};
use super::sub_agent_task_prompt::push_sub_agent_task_system_dynamic;
use super::sub_agent_stream::{run_sub_agent_stream_round, SubAgentStreamOutcome};
use super::sub_message::SubMessageLinkage;
use super::util::new_id;

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

pub(crate) async fn run_sub_agent(
    ctx: &mut super::context::SubAgentLoopContext<'_>,
) -> Result<AgentRunResult> {
    let provider = ctx.provider;
    let state = ctx.session.state;
    let stream = ctx.session.stream;
    let conversation_id = ctx.session.conversation_id;
    let parent_task_board_store_key = ctx.parent_task_board_store_key;
    let message_id = ctx.message_id;
    let task = ctx.task;
    let cancel = ctx.session.cancel.clone();
    let run_id = ctx.run_id;
    let instance_scope = AgentInstanceScope::new(run_id, conversation_id, task.agent_id.clone());
    let sub_provider = sub_agent_provider(provider, &task.agent_id);
    let reasoning_in_messages = effective_reasoning_in_messages(&sub_provider.settings);
    let session =
        init_sub_agent_session(
            state,
            &sub_provider,
            conversation_id,
            message_id,
            parent_task_board_store_key,
            task,
            ctx.enabled_skill_ids,
            ctx.spawn_depth,
            ctx.max_spawn_depth,
        )?;
    let def = session.def;
    let session_extras = session.session_extras;
    let task_dynamic_blocks = session.task_dynamic_blocks;
    let tools_system_appendix = session.tools_system_appendix;
    let allowed_tools = session.allowed_tools;
    let allow_agents = session.allow_agents;
    let sub_task_board_key = session.sub_task_board_key;
    let tool_approval_mode = session.tool_approval_mode;
    let mut local_history = session.local_history;
    let spawn_depth = session.spawn_depth;
    let sub_linkage = SubMessageLinkage {
        anchor_message_id: message_id.to_string(),
        trace_id: agent_trace_step_id(&task.id, &def.id),
        task_id: task.id.clone(),
        spawn_depth,
    };
    let max_cap = ctx.sub_tool_budget.cap();
    let tools_appendix_enabled = !tools_system_appendix.is_empty();
    let native_tools = state.tools.openai_tools(&allowed_tools);
    let budget_scope = ToolBudgetExhaustionScope::sub_agent(max_cap, instance_scope.clone());
    let mut content = String::new();
    let mut reasoning = String::new();

    if def.profile == AgentProfile::Computer {
        state
            .computer_state
            .reset_for_new_user_guidance(conversation_id);
    }

    let mut planner_outcome = PlannerRunOutcome::NotApplicable;
    let mut planner_system_dynamic = Vec::new();
    if def.profile == AgentProfile::Computer {
        push_sub_agent_task_system_dynamic(
            &mut planner_system_dynamic,
            &task_dynamic_blocks,
            state.task_board_store.as_ref(),
            &sub_task_board_key,
            task.id.trim(),
        );
    }

    if def.profile == AgentProfile::Computer && sub_provider.settings.computer_standalone_planner_enabled {
        let planner_scoped_id = new_id("planner_msg");
        let planner_placeholder = ChatMessage {
            id: planner_scoped_id.clone(),
            role: Role::Assistant,
            content: String::new(),
            status: "streaming".into(),
            created_at: super::util::now_ms(),
            tool_calls: None,
            tool_call_id: None,
            error_message: None,
            reasoning: None,
            thoughts: Some(crate::task_board::planner::PLANNER_PHASE_THOUGHTS.into()),
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
            anchor_message_id: Some(sub_linkage.anchor_message_id.clone()),
            trace_id: Some(sub_linkage.trace_id.clone()),
            task_id: Some(sub_linkage.task_id.clone()),
            spawn_depth: Some(sub_linkage.spawn_depth),
        };
        super::sub_message::persist_sub_message(conversation_id, &sub_linkage, &planner_placeholder);
        emit(
            stream,
            StreamEvent::SubMessageStart {
                conversation_id: conversation_id.to_string(),
                anchor_message_id: sub_linkage.anchor_message_id.clone(),
                scoped_message_id: planner_scoped_id.clone(),
                trace_id: sub_linkage.trace_id.clone(),
                task_id: sub_linkage.task_id.clone(),
                spawn_depth: sub_linkage.spawn_depth,
            },
        );
        let planner_ui = crate::task_board::planner::PlannerUiTarget {
            stream,
            state,
            message_id,
            trace_id: Some(sub_linkage.trace_id.as_str()),
            scoped_message_id: Some(planner_scoped_id.as_str()),
        };
        planner_outcome = crate::task_board::planner::run_planner_loop(
            crate::task_board::planner::PlannerRunInput {
                state,
                provider: &sub_provider,
                settings: &sub_provider.settings,
                main_history: &mut local_history,
                conversation_id,
                store_key: &sub_task_board_key,
                lead_agent_id: &def.id,
                lead_profile: def.profile.clone(),
                cancel: &cancel,
                llm_stats: ctx.llm_stats,
                run_id,
                stream,
                context: crate::task_board::planner::PlannerContext::SubAgent {
                    anchor_message_id: message_id.to_string(),
                    trace_id: sub_linkage.trace_id.clone(),
                },
                system_dynamic: &planner_system_dynamic,
                ui: Some(planner_ui),
            },
        )
        .await;
        if matches!(
            planner_outcome,
            PlannerRunOutcome::Planned { .. }
        ) {
            let doc = state.task_board_store.document(&sub_task_board_key);
            if !doc.board_is_empty() {
                super::emit::emit_task_board_updated(
                    stream,
                    conversation_id,
                    &sub_task_board_key,
                    Some(sub_linkage.trace_id.clone()),
                    doc.to_value(),
                );
            }
        }
    }

    // Set thread-local for this sub-agent's tool calls; restore parent on exit.
    let _agent_guard = crate::tools::file::AgentWorkspaceGuard::enter(
        &sub_provider.settings.workspace_root,
    );

    let planner_bundle =
        def.profile == AgentProfile::Computer && sub_provider.settings.computer_standalone_planner_enabled;
    let work_items_enabled = planner_bundle;
    let b42_enforced = planner_bundle;
    let computer_no_exec_init = planner_bundle;

    loop {
        match agent_round_lifecycle::check_loop_guards(&cancel, ctx.sub_tool_budget) {
            agent_round_lifecycle::LoopGuardOutcome::Continue => {}
            agent_round_lifecycle::LoopGuardOutcome::Cancelled => {
                state.computer_state.mark_cancelled(conversation_id);
                let toast_msg = if def.profile == AgentProfile::Computer {
                    "计算机操作已取消"
                } else {
                    "子 Agent 已停止"
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

        let round_message_id = new_id("agent_msg");
        let round_placeholder = ChatMessage {
            id: round_message_id.clone(),
            role: Role::Assistant,
            content: String::new(),
            status: "streaming".into(),
            created_at: super::util::now_ms(),
            tool_calls: None,
            tool_call_id: None,
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
            },
        );

        let round_prompts = prepare_sub_agent_round_prompts(super::context::SubAgentPromptContext {
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
            planner_outcome: planner_outcome.clone(),
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
        };
        let mut stream_refs = super::context::SubStreamRoundRefs {
            task,
            def: &def,
            instance_scope: &instance_scope,
            message_id,
            round_message_id: &round_message_id,
            session_content: &mut content,
            local_history: &mut local_history,
            llm_stats: ctx.llm_stats,
        };
        let stream_input = super::context::StreamRoundInput {
            history_for_api: round_prompts.history_for_api,
            system_prompts: round_prompts.system_prompts,
            native_tools: native_tools.clone(),
            tools_appendix_enabled,
        };
        let stream_outcome = run_sub_agent_stream_round(
            &mut stream_ctx,
            &mut stream_refs,
            stream_input,
        )
        .await?;

        let buf = match stream_outcome {
            SubAgentStreamOutcome::RetryAfterRecoveryHint => continue,
            SubAgentStreamOutcome::Completed(b) => b,
        };

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

        if reasoning_in_messages {
            reasoning.push_str(&buf.reasoning_buf);
        }

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
                    reasoning_in_messages,
                    reasoning,
                ));
            }
            PostAssistantTurnAction::ExecuteTools => {}
        }

        let mut pipeline_before_capture = None;
        if verify_host_active(state, &def.profile)
            && batch_has_desktop_root_tool(&buf.final_tool_calls, state.tools.as_ref())
        {
            pipeline_before_capture = Some(
                ensure_verify_before_capture(state, conversation_id).await?,
            );
        }

        let sub_cfg = SubToolPassConfig {
            def: &def,
            task,
            allowed_tools: &allowed_tools,
            allow_agents: &allow_agents,
            instance_scope: &instance_scope,
            agent_trace: ctx.agent_trace,
            accumulated_content: content.clone(),
            accumulated_reasoning: reasoning.clone(),
            reasoning_in_messages,
            trace_id: sub_linkage.trace_id.clone(),
            spawn_depth,
            scoped_message_id: round_message_id.clone(),
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
                task_board_b42_enforced: b42_enforced,
                task_board_computer_no_exec_init: computer_no_exec_init,
                workspace_root: &sub_provider.settings.workspace_root,
            },
            final_tool_calls: &buf.final_tool_calls,
            trim_hook: Some(trim_hook),
            cancel: cancel.clone(),
        };
        match Box::pin(run_agent_tool_pass(pass)).await?
        {
            ToolPassResult::SubFinished(result) => return Ok(result),
            ToolPassResult::FinalReplyComplete(output) => {
                return Ok(sub_agent_run_result(
                    &task.id,
                    &def,
                    output,
                    reasoning_in_messages,
                    reasoning,
                ));
            }
            ToolPassResult::NoopExit => {
                return Ok(sub_agent_run_result(
                    &task.id,
                    &def,
                    sub_agent_handoff_content(&local_history, &content),
                    reasoning_in_messages,
                    reasoning,
                ));
            }
            ToolPassResult::RanTools => {}
        }

        if verify_host_active(state, &def.profile)
            && batch_has_desktop_root_tool(&buf.final_tool_calls, state.tools.as_ref())
        {
            let mut pipeline_usage = PipelineLlmUsageRecorder {
                stats: ctx.llm_stats,
                scope: &instance_scope,
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
            let (_last_op, last_verify) =
                state.computer_state.pipeline_context_fields(conversation_id);
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
