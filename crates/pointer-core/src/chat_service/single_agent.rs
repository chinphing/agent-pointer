//! Single-agent streaming loop: prompt hooks per round, then `single_agent_stream` + tools.

use crate::agents::{AgentPlan, AgentProfile};
use crate::llm_token_stats::ChatLlmTokenSession;
use crate::models::{effective_max_tokens, ChatMessage, ModelSettings, StreamEvent};
use crate::provider::OpenAIProvider;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::app_state::AppState;
use super::emit::emit;
use super::session_budget::SessionToolBudget;
use super::util::new_id;
use super::StreamTx;

pub(super) async fn run_single_agent_loop(
    stream: StreamTx,
    state: Arc<AppState>,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    enabled_skill_ids: &[String],
    agent_plan: &AgentPlan,
    provider: &OpenAIProvider,
    settings: &ModelSettings,
    tool_approval_mode: &str,
    tool_budget: &mut SessionToolBudget,
    consumed_single: &mut u32,
    max_cap: u32,
    cancel: CancellationToken,
    llm_token_session: &mut ChatLlmTokenSession,
    reasoning_in_messages: bool,
) -> Result<()> {
    loop {
        if cancel.is_cancelled() {
            tool_budget.sync_out(consumed_single);
            return Err(anyhow!("已停止生成"));
        }

        if tool_budget.remaining() == 0 {
            tool_budget.sync_out(consumed_single);
            return Err(anyhow!(
                "本会话单智能体工具调用轮次已达上限（{}）。请新开对话。",
                max_cap
            ));
        }

        let assistant_id = new_id("msg");

        let lead_profile = state
            .agents
            .get(&agent_plan.lead_agent_id)
            .map(|a| a.def().profile.clone())
            .unwrap_or(AgentProfile::General);
        let computer_positioning = if lead_profile == AgentProfile::Computer {
            Some(crate::agents::computer::tools::tool_prompts::positioning_mode_for_tier(
                state.computer_state.tier_for_conversation(conversation_id),
            ))
        } else {
            None
        };
        let tools_system_appendix =
            crate::tools_system_appendix::generate_tools_system_appendix_with_positioning(
                &state.tools,
                &agent_plan.allowed_tool_names,
                computer_positioning,
            );
        let tools_appendix_enabled = !tools_system_appendix.is_empty();
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
            &state,
            &stream,
            conversation_id,
            history,
            agent_plan,
            settings,
            &assistant_id,
            lead_profile.clone(),
            tools_system_appendix,
            tools_appendix_enabled,
        )
        .await?;

        let round_settings = if lead_profile == AgentProfile::Computer {
            state.computer_state.apply_round_settings(conversation_id, settings)
        } else {
            settings.clone()
        };

        let stream_outcome = super::single_agent_stream::run_provider_stream_round(
            stream.clone(),
            state.clone(),
            provider,
            &round_settings,
            conversation_id,
            history,
            llm_token_session,
            assistant_id.clone(),
            tools_appendix_enabled,
            tool_budget,
            consumed_single,
            max_cap,
            cancel.clone(),
            reasoning_in_messages,
            round_prompts.history_for_api,
            round_prompts.system_prompts,
        )
        .await?;
        let buf = match stream_outcome {
            super::single_agent_stream::ProviderRoundOutcome::RetryAfterRecoveryHint => continue,
            super::single_agent_stream::ProviderRoundOutcome::Completed(b) => b,
        };

        let lead_scope = llm_token_session.lead_scope.clone();
        let lead_instance_id = Some(lead_scope.agent_instance_id.clone());
        let assistant_msg = super::single_agent_post_stream::build_assistant_message_after_stream(
            &assistant_id,
            buf.raw_content_buf.as_str(),
            buf.reasoning_buf,
            reasoning_in_messages,
            &buf.final_tool_calls,
            buf.xml_thoughts,
            buf.xml_headline,
            agent_plan,
            lead_instance_id,
            &agent_trace,
            state.as_ref(),
        );
        super::single_agent_post_stream::commit_assistant_turn(
            &stream,
            history,
            &assistant_id,
            &assistant_msg,
        );

        if lead_profile == AgentProfile::Computer {
            state.computer_state.on_assistant_round_complete(
                conversation_id,
                assistant_msg.thoughts.as_deref(),
            );
        }

        let post_action = if buf.final_tool_calls.is_empty() {
            super::single_agent_post_stream::decide_when_no_tool_calls(
                &stream,
                &state,
                history,
                settings,
                provider,
                conversation_id,
                &cancel,
                tool_budget,
                consumed_single,
                max_cap,
                lead_scope.clone(),
                &assistant_id,
                &buf.json_finish_diag,
                tools_appendix_enabled,
                &buf.finish_reason,
                effective_max_tokens(settings),
            )
            .await?
        } else {
            super::single_agent_post_stream::decide_when_tool_calls_present(
                &stream,
                &state,
                state.tools.as_ref(),
                history,
                settings,
                provider,
                conversation_id,
                &cancel,
                tool_budget,
                consumed_single,
                max_cap,
                lead_scope.clone(),
                &buf.final_tool_calls,
            )
            .await?
        };

        match post_action {
            super::single_agent_post_stream::PostAssistantTurnAction::FinishRun => {
                return Ok(());
            }
            super::single_agent_post_stream::PostAssistantTurnAction::RetryLoop => continue,
            super::single_agent_post_stream::PostAssistantTurnAction::ExecuteTools => {}
        }

        match super::single_agent_tools::run_single_agent_tool_pass(
            stream.clone(),
            state.as_ref(),
            conversation_id,
            history,
            &agent_plan.allow_agents,
            enabled_skill_ids,
            provider,
            tool_approval_mode,
            tool_budget,
            consumed_single,
            cancel.clone(),
            llm_token_session,
            reasoning_in_messages,
            assistant_id.clone(),
            file_tool_lead_for_invoke.clone(),
            &buf.final_tool_calls,
            buf.raw_content_buf.as_str(),
            &mut agent_trace,
            settings,
            &agent_plan.lead_agent_id,
        )
        .await?
        {
            super::single_agent_tools::ToolPassResult::Finished
            | super::single_agent_tools::ToolPassResult::NoopExit => return Ok(()),
            super::single_agent_tools::ToolPassResult::RanTools => {}
        }
        tool_budget.record_tool_cycle();
        tool_budget.sync_out(consumed_single);
        super::single_agent_post_stream::bail_on_tool_budget_exhausted(
            &stream,
            &state,
            history,
            settings,
            provider,
            conversation_id,
            &cancel,
            tool_budget,
            consumed_single,
            max_cap,
            lead_scope,
        )
        .await?;
    }
}
