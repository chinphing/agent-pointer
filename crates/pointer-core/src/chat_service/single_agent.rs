//! Single-agent streaming loop: prompt hooks per round, then `single_agent_stream` + tools.

use crate::agents::{AgentPlan, AgentProfile};
use crate::llm_token_stats::ChatLlmTokenSession;
use crate::models::{ChatMessage, ModelSettings, StreamEvent};
use crate::provider::OpenAIProvider;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::app_state::AppState;
use super::emit::emit;
use super::session_budget::SessionToolBudget;
use super::util::new_id;
use super::StreamTx;

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
    stream: StreamTx,
    state: Arc<AppState>,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    enabled_skill_ids: &mut Vec<String>,
    agent_plan: &AgentPlan,
    provider: &OpenAIProvider,
    settings: &ModelSettings,
    main_task_board_store_key: &str,
    tool_approval_mode: &str,
    tool_budget: &mut SessionToolBudget,
    consumed_single: &mut u32,
    max_cap: u32,
    cancel: CancellationToken,
    llm_token_session: &mut ChatLlmTokenSession,
    reasoning_in_messages: bool,
) -> Result<()> {
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

    loop {
        match super::agent_round_lifecycle::check_loop_guards(&cancel, tool_budget) {
            super::agent_round_lifecycle::LoopGuardOutcome::Continue => {}
            super::agent_round_lifecycle::LoopGuardOutcome::Cancelled => {
                tool_budget.sync_out(consumed_single);
                return Err(anyhow!("已停止生成"));
            }
            super::agent_round_lifecycle::LoopGuardOutcome::BudgetExhausted => {
                tool_budget.sync_out(consumed_single);
                return Err(anyhow!(
                    "本会话单智能体工具调用轮次已达上限（{}）。请新开对话。",
                    max_cap
                ));
            }
        }

        let assistant_id = new_id("msg");

        let effective_allowed = agent_plan.allowed_tool_names.clone();
        let tools_system_appendix =
            crate::tools_system_appendix::generate_tools_system_appendix(
                &state.tools,
                &effective_allowed,
            );
        let tools_appendix_enabled = !tools_system_appendix.is_empty();
        let native_tools = state.tools.openai_tools(&effective_allowed);
        let file_tool_lead_for_invoke = lead_profile.clone();

        emit(
            &stream,
            StreamEvent::MessageStart {
                message_id: assistant_id.clone(),
                conversation_id: conversation_id.to_string(),
            },
        );

        let mut agent_trace = Vec::new();

        let round_prompts = super::single_agent_prompt::prepare_single_agent_round_prompts(
            super::context::SingleAgentPromptContext {
                session: super::context::SessionRefsArc {
                    stream: &stream,
                    state: state.clone(),
                    conversation_id,
                    cancel: cancel.clone(),
                },
                history,
                agent_plan,
                settings,
                main_task_board_store_key,
                assistant_id: &assistant_id,
                lead_profile: lead_profile.clone(),
                tools_system_appendix,
                tools_appendix_enabled,
            },
        )
        .await?;

        let round_settings = if lead_profile == AgentProfile::Computer {
            state.computer_state.apply_round_settings(conversation_id, settings)
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
            history,
            token_session: llm_token_session,
            tool_budget,
            consumed_single,
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
            super::single_agent_stream::ProviderRoundOutcome::RetryAfterRecoveryHint => continue,
            super::single_agent_stream::ProviderRoundOutcome::Completed(b) => b,
        };

        let lead_scope = llm_token_session.lead_scope.clone();
        let lead_instance_id = Some(lead_scope.agent_instance_id.clone());
        let mut assistant_msg = super::single_agent_post_stream::build_assistant_message_after_stream(
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
            assistant_msg.tool_raw_output = latest_round_tool_raw_output(history);
        }
        super::single_agent_post_stream::commit_assistant_turn(
            &stream,
            conversation_id,
            history,
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
            tool_budget.sync_out(consumed_single);
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
                history,
                provider,
                settings,
                tool_budget,
                Some(consumed_single),
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

        match super::single_agent_tools::run_single_agent_tool_pass(
            super::agent_tool_pass::LeadSingleToolPassRequest {
                session: super::context::SessionRefs {
                    stream: &stream,
                    state: state.as_ref(),
                    conversation_id,
                    cancel: &cancel,
                },
                main_task_board_store_key,
                history,
                allow_agents: &agent_plan.allow_agents,
                enabled_skill_ids,
                provider,
                tool_approval_mode,
                tool_budget,
                consumed_single,
                token_session: llm_token_session,
                settings,
                lead_agent_id: &agent_plan.lead_agent_id,
                file_tool_lead_for_invoke: file_tool_lead_for_invoke.clone(),
                assistant_id: assistant_id.clone(),
                final_tool_calls: &buf.final_tool_calls,
                agent_trace: &mut agent_trace,
                cancel: cancel.clone(),
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
                    history,
                    &delivery_id,
                    &delivery_msg,
                );
                tool_budget.sync_out(consumed_single);
                return Ok(());
            }
            super::single_agent_tools::ToolPassResult::RanTools => {}
        }
        {
            let mut post_ctx = super::context::PostAssistantContext::new(
                &stream,
                state.as_ref(),
                conversation_id,
                &cancel,
                history,
                provider,
                settings,
                tool_budget,
                Some(consumed_single),
                max_cap,
                &budget_scope,
            );
            super::agent_round_lifecycle::finish_tool_round_cycle(&mut post_ctx).await?;
        }
    }
}
