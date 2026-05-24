//! Sub-agent (`run_sub_agent`) tool loop: isolated history, shared stream/post-stream/tool-pass with lead.

use anyhow::{anyhow, Result};
use tokio_util::sync::CancellationToken;

use crate::agent_instance_scope::AgentInstanceScope;
use crate::agents::{AgentProfile, AgentRunResult, AgentTask};
use crate::llm_token_stats::ConversationLlmStats;
use crate::models::{effective_max_tokens, effective_reasoning_in_messages, AgentTrace, StreamEvent};
use crate::provider::OpenAIProvider;

use super::agent_post_stream::{
    bail_on_tool_budget_exhausted, build_sub_assistant_message_after_stream,
    decide_when_no_tool_calls, decide_when_tool_calls_present, push_sub_assistant_turn,
    sub_agent_run_result, FormatRetryDelivery, PostAssistantTurnAction, ToolBudgetExhaustionScope,
};
use super::agent_tool_pass::{
    run_agent_tool_pass, SubToolPassConfig, ToolInvocationStats, ToolPassResult,
};
use crate::task_board::TaskBoardTrimHook;
use super::app_state::AppState;
use super::emit::{agent_trace_step_id, emit, trace_id_opt};
use super::session_budget::SessionToolBudget;
use super::session_model::sub_agent_provider;
use super::sub_agent_prompt::{init_sub_agent_session, prepare_sub_agent_round_prompts};
use super::sub_agent_stream::{run_sub_agent_stream_round, SubAgentStreamOutcome};
use super::util::new_id;
use super::StreamTx;

pub(crate) async fn run_sub_agent(
    provider: &OpenAIProvider,
    state: &AppState,
    stream: &StreamTx,
    conversation_id: &str,
    message_id: &str,
    agent_trace: &mut Vec<AgentTrace>,
    enabled_skill_ids: &[String],
    task: &AgentTask,
    sub_tool_budget: &mut SessionToolBudget,
    cancel: CancellationToken,
    _reasoning_in_messages: bool,
    llm_stats: &mut ConversationLlmStats,
) -> Result<AgentRunResult> {
    let instance_scope = AgentInstanceScope::new(conversation_id, task.agent_id.clone());
    let sub_provider = sub_agent_provider(provider, &task.agent_id);
    let reasoning_in_messages = effective_reasoning_in_messages(&sub_provider.settings);
    let session =
        init_sub_agent_session(state, &sub_provider, conversation_id, task, enabled_skill_ids)?;
    let def = session.def;
    let prompts = session.prompts;
    let tools_system_appendix = session.tools_system_appendix;
    let allowed_tools = session.allowed_tools;
    let sub_task_board_key = session.sub_task_board_key;
    let tool_approval_mode = session.tool_approval_mode;
    let mut local_history = session.local_history;
    let max_cap = sub_tool_budget.cap();
    let tools_appendix_enabled = !tools_system_appendix.is_empty();
    let budget_scope = ToolBudgetExhaustionScope::sub_agent(max_cap, instance_scope.clone());
    let mut content = String::new();
    let mut reasoning = String::new();

    loop {
        if cancel.is_cancelled() {
            state.computer_state.mark_cancelled(conversation_id);
            return Err(anyhow!("已停止生成"));
        }

        if sub_tool_budget.remaining() == 0 {
            state.computer_state.mark_cancelled(conversation_id);
            return Err(anyhow!(
                "子 Agent 工具调用轮次已达上限（{}）。请新开对话或在设置中调高上限。",
                max_cap
            ));
        }

        let round_message_id = new_id("agent_msg");
        let round_prompts = prepare_sub_agent_round_prompts(
            state,
            stream,
            conversation_id,
            message_id,
            &task.id,
            &round_message_id,
            &local_history,
            &prompts,
            &tools_system_appendix,
            &sub_task_board_key,
            &def,
        )
        .await?;

        let stream_outcome = run_sub_agent_stream_round(
            stream,
            state,
            &sub_provider,
            conversation_id,
            message_id,
            task,
            &def,
            &instance_scope,
            agent_trace,
            &mut content,
            reasoning_in_messages,
            llm_stats,
            &mut local_history,
            sub_tool_budget,
            max_cap,
            tools_appendix_enabled,
            cancel.clone(),
            round_prompts.history_for_api,
            round_prompts.system_prompts,
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
                thoughts: None,
                headline: None,
                trace_id: trace_id_opt(Some(&agent_trace_step_id(&task.id, &def.id))),
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
            buf.xml_headline,
            &def,
            Some(instance_scope.agent_instance_id.clone()),
            state,
        );
        push_sub_assistant_turn(&mut local_history, assistant_msg);

        if def.profile == AgentProfile::Computer {
            state.computer_state.on_assistant_round_complete(
                conversation_id,
                local_history
                    .last()
                    .and_then(|m| m.thoughts.as_deref()),
            );
        }

        let post_action = if buf.final_tool_calls.is_empty() {
            decide_when_no_tool_calls(
                FormatRetryDelivery::LocalHistoryOnly,
                stream,
                state,
                &mut local_history,
                &sub_provider.settings,
                &sub_provider,
                conversation_id,
                &cancel,
                sub_tool_budget,
                None,
                max_cap,
                &budget_scope,
                &round_message_id,
                &buf.json_finish_diag,
                tools_appendix_enabled,
                &buf.finish_reason,
                effective_max_tokens(&sub_provider.settings),
            )
            .await?
        } else {
            decide_when_tool_calls_present(
                FormatRetryDelivery::LocalHistoryOnly,
                stream,
                state,
                state.tools.as_ref(),
                &mut local_history,
                &sub_provider.settings,
                &sub_provider,
                conversation_id,
                &cancel,
                sub_tool_budget,
                None,
                max_cap,
                &budget_scope,
                &buf.final_tool_calls,
                "sub-agent",
            )
            .await?
        };

        match post_action {
            PostAssistantTurnAction::FinishRun => {
                return Ok(sub_agent_run_result(
                    &task.id,
                    &def,
                    content,
                    reasoning_in_messages,
                    reasoning,
                ));
            }
            PostAssistantTurnAction::RetryLoop => continue,
            PostAssistantTurnAction::ExecuteTools => {}
        }

        let sub_cfg = SubToolPassConfig {
            def: &def,
            task,
            allowed_tools: &allowed_tools,
            round_message_id: &round_message_id,
            accumulated_content: content.clone(),
            accumulated_reasoning: reasoning.clone(),
            reasoning_in_messages,
            trace_id: agent_trace_step_id(&task.id, &def.id),
        };
        let mut stats = ToolInvocationStats::Conversation(llm_stats);
        let trim_hook = TaskBoardTrimHook {
            settings: &sub_provider.settings,
            agent_id: &def.id,
            conversation_id,
            stream: &stream,
            emit_history_replaced: false,
        };
        match Box::pin(run_agent_tool_pass(
            stream.clone(),
            state,
            conversation_id,
            message_id.to_string(),
            &mut local_history,
            &tool_approval_mode,
            sub_tool_budget,
            None,
            cancel.clone(),
            &sub_provider,
            &sub_task_board_key,
            &mut stats,
            &buf.final_tool_calls,
            None,
            Some(sub_cfg),
            Some(trim_hook),
        ))
        .await?
        {
            ToolPassResult::SubFinished(result) => return Ok(result),
            ToolPassResult::NoopExit => {
                return Ok(sub_agent_run_result(
                    &task.id,
                    &def,
                    content,
                    reasoning_in_messages,
                    reasoning,
                ));
            }
            ToolPassResult::LeadFinished | ToolPassResult::RanTools => {}
        }

        sub_tool_budget.record_tool_cycle();
        bail_on_tool_budget_exhausted(
            stream,
            state,
            &mut local_history,
            &sub_provider.settings,
            &sub_provider,
            conversation_id,
            &cancel,
            sub_tool_budget,
            None,
            max_cap,
            &budget_scope,
        )
        .await?;
    }
}
