//! Sub-agent (`run_sub_agent`) tool loop: isolated history, shared stream/post-stream/tool-pass with lead.

use anyhow::{anyhow, Result};
use tokio_util::sync::CancellationToken;

use crate::agent_instance_scope::AgentInstanceScope;
use crate::agents::{AgentProfile, AgentRunResult, AgentTask};
use crate::llm_token_stats::ConversationLlmStats;
use crate::models::{effective_reasoning_in_messages, AgentTrace, ChatMessage, Role, StreamEvent};
use crate::provider::OpenAIProvider;

use super::agent_post_stream::{
    bail_on_tool_budget_exhausted, build_sub_assistant_message_after_stream,
    decide_when_no_tool_calls, decide_when_tool_calls_present, push_sub_assistant_turn,
    sub_agent_run_result, PostAssistantTurnAction, ToolBudgetExhaustionScope,
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
    provider: &OpenAIProvider,
    state: &AppState,
    stream: &StreamTx,
    conversation_id: &str,
    parent_task_board_store_key: &str,
    message_id: &str,
    agent_trace: &mut Vec<AgentTrace>,
    enabled_skill_ids: &[String],
    task: &AgentTask,
    sub_tool_budget: &mut SessionToolBudget,
    cancel: CancellationToken,
    _reasoning_in_messages: bool,
    llm_stats: &mut ConversationLlmStats,
    run_id: &str,
) -> Result<AgentRunResult> {
    let instance_scope = AgentInstanceScope::new(run_id, conversation_id, task.agent_id.clone());
    let sub_provider = sub_agent_provider(provider, &task.agent_id);
    let reasoning_in_messages = effective_reasoning_in_messages(&sub_provider.settings);
    let session =
        init_sub_agent_session(
            state,
            &sub_provider,
            conversation_id,
            parent_task_board_store_key,
            task,
            enabled_skill_ids,
        )?;
    let def = session.def;
    let session_extras = session.session_extras;
    let tools_system_appendix = session.tools_system_appendix;
    let allowed_tools = session.allowed_tools;
    let allow_agents = session.allow_agents;
    let sub_task_board_key = session.sub_task_board_key;
    let tool_approval_mode = session.tool_approval_mode;
    let mut local_history = session.local_history;
    let max_cap = sub_tool_budget.cap();
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

    loop {
        if cancel.is_cancelled() {
            state.computer_state.mark_cancelled(conversation_id);
            let toast_msg = if def.profile == AgentProfile::Computer {
                "计算机操作已取消"
            } else {
                "子 Agent 已停止"
            };
            let _ = stream.send(StreamEvent::UiToast {
                conversation_id: conversation_id.to_string(),
                message: toast_msg.to_string(),
                level: "warning".to_string(),
            });
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
            &session_extras,
            &tools_system_appendix,
            &sub_task_board_key,
            &def,
            sub_provider.settings.workspace_root.as_str(),
            sub_provider.settings.user_dynamic_inject_enabled,
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
            native_tools.clone(),
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
                tool_raw_output: None,
                thoughts: None,
                headline: None,
                trace_id: trace_id_opt(Some(&agent_trace_step_id(&task.id, &def.id))),
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
        push_sub_assistant_turn(&mut local_history, assistant_msg);

        if def.profile == AgentProfile::Computer {
            let last_msg = local_history.last();
            state.computer_state.on_assistant_round_complete(
                conversation_id,
                last_msg.and_then(|m| m.thoughts.as_deref()),
                last_msg.and_then(|m| m.tool_calls.as_deref()),
            );
            if state.computer_state.should_give_up(conversation_id) {
                state.computer_state.mark_cancelled(conversation_id);
                return Err(anyhow!(
                    "当前任务已尽力但仍无法完成（重复操作达到 {} 次），请提供进一步指导。",
                    crate::agents::computer::tier::GIVE_UP_THRESHOLD
                ));
            }
        }

        let post_action = if buf.final_tool_calls.is_empty() {
            decide_when_no_tool_calls(
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
            .await?
        } else {
            decide_when_tool_calls_present(
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
                        Some(message_id.to_string()),
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

        let sub_cfg = SubToolPassConfig {
            def: &def,
            task,
            allowed_tools: &allowed_tools,
            allow_agents: &allow_agents,
            instance_scope: &instance_scope,
            agent_trace,
            accumulated_content: content.clone(),
            accumulated_reasoning: reasoning.clone(),
            reasoning_in_messages,
            trace_id: agent_trace_step_id(&task.id, &def.id),
        };
        let mut stats = ToolInvocationStats::Conversation(llm_stats);
        let anchor_message_id =
            state.get_main_task_board_anchor(conversation_id, &sub_task_board_key);
        let trim_hook = TaskBoardTrimHook {
            settings: &sub_provider.settings,
            agent_id: &def.id,
            conversation_id,
            stream: &stream,
            emit_history_replaced: false,
            anchor_message_id: anchor_message_id.as_deref(),
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
