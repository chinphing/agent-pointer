//! After one provider stream round: assistant row, format retries, tool-budget exhaustion.
//! Shared by lead single-agent and sub-agent loops.

use crate::agents::{AgentDef, AgentPlan, AgentRunResult};
use crate::json_tool_caller::JsonToolFinishDiagnostics;
use crate::models::{AgentTrace, ChatMessage, ModelSettings, Role, StreamEvent, ToolCall};
use crate::provider::OpenAIProvider;
use crate::tools::parse_tool_call_arguments;
use crate::tools::ToolRegistry;
use anyhow::{anyhow, Result};
use tokio_util::sync::CancellationToken;

use super::app_state::AppState;
use super::content_extract::extract_user_visible_content;
use super::emit::emit;
use super::json_tool_retries::{
    json_tool_empty_calls_retry_message, json_tool_envelope_batch_retry_message,
    push_injected_format_retry_turn, rollback_failed_json_assistant_turn,
};
use super::session_budget::SessionToolBudget;
use super::util::{new_id, now_ms};
use super::StreamTx;

/// What the outer agent loop should do after persisting the assistant turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PostAssistantTurnAction {
    /// No tools and no format retry — end the run successfully.
    FinishRun,
    /// Format-only retry injected — continue the outer loop without consuming a tool round.
    RetryLoop,
    /// Valid tool batch — run tool pass.
    ExecuteTools,
}

/// How to deliver a format-retry user line into history / UI.
pub(super) enum FormatRetryDelivery<'a> {
    /// Lead session: emit `InjectedUserMessage` and append to main history.
    InjectedUser {
        stream: &'a StreamTx,
        conversation_id: &'a str,
    },
    /// Sub-agent: append to `local_history` only (no injected-user event).
    LocalHistoryOnly,
}

/// Tool-round budget exhaustion copy and compression behavior.
pub(super) struct ToolBudgetExhaustionScope {
    pub user_hint: String,
    pub error_message: String,
    pub compress_for_session: bool,
}

impl ToolBudgetExhaustionScope {
    pub(super) fn lead_single(max_cap: u32) -> Self {
        Self {
            user_hint: format!(
                "单智能体模式下工具调用累计已达上限（{} 轮，含此前消息）。建议新开对话；将尝试压缩上下文以便查看摘要。",
                max_cap
            ),
            error_message: format!(
                "单智能体模式下工具调用轮次已达上限（{max_cap}）。请新开对话或在设置中调高上限。"
            ),
            compress_for_session: true,
        }
    }

    pub(super) fn sub_agent(max_cap: u32) -> Self {
        Self {
            user_hint: format!(
                "子 Agent 内工具调用累计已达上限（{} 轮）。建议新开对话。",
                max_cap
            ),
            error_message: format!(
                "子 Agent 内工具调用轮次已达上限（{max_cap}）。请新开对话。"
            ),
            compress_for_session: false,
        }
    }
}

fn push_format_retry_user_line(
    delivery: FormatRetryDelivery<'_>,
    history: &mut Vec<ChatMessage>,
    hint: String,
) {
    match delivery {
        FormatRetryDelivery::InjectedUser {
            stream,
            conversation_id,
        } => push_injected_format_retry_turn(stream, conversation_id, history, hint),
        FormatRetryDelivery::LocalHistoryOnly => {
            history.push(ChatMessage {
                id: new_id("fmt_retry"),
                role: Role::User,
                content: hint,
                status: "done".into(),
                created_at: now_ms(),
                tool_calls: None,
                tool_call_id: None,
                error_message: None,
                reasoning: None,
                thoughts: None,
                headline: None,
                raw_content: None,
                agent_id: None,
                agent_name: None,
                agent_trace: None,
                images_base64: None,
                computer_round_screen_rel_path: None,
            });
        }
    }
}

fn assistant_tool_calls_with_risk(
    final_tool_calls: &[ToolCall],
    state: &AppState,
) -> Option<Vec<ToolCall>> {
    if final_tool_calls.is_empty() {
        return None;
    }
    Some(
        final_tool_calls
            .iter()
            .cloned()
            .map(|mut t| {
                let args_v = parse_tool_call_arguments(&t.arguments);
                t.risk_level = state
                    .tools
                    .tool_risk_level_for_invocation(&t.name, &args_v)
                    .or(Some("low".into()));
                t
            })
            .collect(),
    )
}

pub(super) fn build_lead_assistant_message_after_stream(
    assistant_id: &str,
    raw_content_buf: &str,
    reasoning_buf: String,
    reasoning_in_messages: bool,
    final_tool_calls: &[ToolCall],
    xml_thoughts: Option<String>,
    xml_headline: Option<String>,
    agent_plan: &AgentPlan,
    agent_trace: &[AgentTrace],
    state: &AppState,
) -> ChatMessage {
    ChatMessage {
        id: assistant_id.to_string(),
        role: Role::Assistant,
        content: extract_user_visible_content(raw_content_buf),
        status: if final_tool_calls.is_empty() {
            "completed".into()
        } else {
            "streaming".into()
        },
        created_at: now_ms(),
        tool_calls: assistant_tool_calls_with_risk(final_tool_calls, state),
        tool_call_id: None,
        error_message: None,
        reasoning: if reasoning_in_messages && !reasoning_buf.is_empty() {
            Some(reasoning_buf)
        } else {
            None
        },
        thoughts: xml_thoughts,
        headline: xml_headline,
        raw_content: if raw_content_buf.is_empty() {
            None
        } else {
            Some(raw_content_buf.to_string())
        },
        agent_id: Some(agent_plan.lead_agent_id.clone()),
        agent_name: Some(agent_plan.lead_agent_name.clone()),
        agent_trace: if agent_trace.is_empty() {
            None
        } else {
            Some(agent_trace.to_vec())
        },
        images_base64: None,
        computer_round_screen_rel_path: None,
    }
}

pub(super) fn build_sub_assistant_message_after_stream(
    round_message_id: &str,
    round_content: String,
    round_reasoning: String,
    reasoning_in_messages: bool,
    final_tool_calls: &[ToolCall],
    round_thoughts: Option<String>,
    round_headline: Option<String>,
    def: &AgentDef,
    state: &AppState,
) -> ChatMessage {
    ChatMessage {
        id: round_message_id.to_string(),
        role: Role::Assistant,
        content: round_content,
        status: if final_tool_calls.is_empty() {
            "completed".into()
        } else {
            "streaming".into()
        },
        created_at: now_ms(),
        tool_calls: assistant_tool_calls_with_risk(final_tool_calls, state),
        tool_call_id: None,
        error_message: None,
        reasoning: if reasoning_in_messages && !round_reasoning.is_empty() {
            Some(round_reasoning)
        } else {
            None
        },
        thoughts: round_thoughts,
        headline: round_headline,
        raw_content: None,
        agent_id: Some(def.id.clone()),
        agent_name: Some(def.name.clone()),
        agent_trace: None,
        images_base64: None,
        computer_round_screen_rel_path: None,
    }
}

pub(super) fn commit_lead_assistant_turn(
    stream: &StreamTx,
    history: &mut Vec<ChatMessage>,
    assistant_id: &str,
    assistant_msg: &ChatMessage,
) {
    history.push(assistant_msg.clone());
    emit(
        stream,
        StreamEvent::MessageEnd {
            message_id: assistant_id.to_string(),
            content: Some(assistant_msg.content.clone()),
            raw_content: assistant_msg.raw_content.clone(),
            thoughts: assistant_msg.thoughts.clone(),
            headline: assistant_msg.headline.clone(),
        },
    );
}

pub(super) fn push_sub_assistant_turn(history: &mut Vec<ChatMessage>, assistant_msg: ChatMessage) {
    history.push(assistant_msg);
}

async fn sync_out_and_bail_if_exhausted(
    stream: &StreamTx,
    state: &AppState,
    history: &mut Vec<ChatMessage>,
    settings: &ModelSettings,
    provider: &OpenAIProvider,
    conversation_id: &str,
    cancel: &CancellationToken,
    tool_budget: &mut SessionToolBudget,
    mut consumed_single: Option<&mut u32>,
    max_cap: u32,
    scope: &ToolBudgetExhaustionScope,
) -> Result<()> {
    if let Some(consumed) = consumed_single.as_mut() {
        tool_budget.sync_out(consumed);
    }
    bail_on_tool_budget_exhausted(
        stream,
        state,
        history,
        settings,
        provider,
        conversation_id,
        cancel,
        tool_budget,
        consumed_single,
        max_cap,
        scope,
    )
    .await
}

/// Empty tool batch: optional JSON format retry, or successful stop.
pub(super) async fn decide_when_no_tool_calls(
    delivery: FormatRetryDelivery<'_>,
    stream: &StreamTx,
    state: &AppState,
    history: &mut Vec<ChatMessage>,
    settings: &ModelSettings,
    provider: &OpenAIProvider,
    conversation_id: &str,
    cancel: &CancellationToken,
    tool_budget: &mut SessionToolBudget,
    mut consumed_single: Option<&mut u32>,
    max_cap: u32,
    scope: &ToolBudgetExhaustionScope,
    assistant_turn_id: &str,
    json_finish_diag: &JsonToolFinishDiagnostics,
    tools_appendix_enabled: bool,
    finish_reason: &str,
    max_tokens: u32,
) -> Result<PostAssistantTurnAction> {
    if let Some(hint) = json_tool_empty_calls_retry_message(
        json_finish_diag,
        tools_appendix_enabled,
        finish_reason,
        max_tokens,
    ) {
        rollback_failed_json_assistant_turn(history, assistant_turn_id);
        push_format_retry_user_line(delivery, history, hint);
        sync_out_and_bail_if_exhausted(
            stream,
            state,
            history,
            settings,
            provider,
            conversation_id,
            cancel,
            tool_budget,
            consumed_single,
            max_cap,
            scope,
        )
        .await?;
        return Ok(PostAssistantTurnAction::RetryLoop);
    }
    let _ = finish_reason;
    if let Some(consumed) = consumed_single.as_mut() {
        tool_budget.sync_out(consumed);
    }
    Ok(PostAssistantTurnAction::FinishRun)
}

/// Non-empty tool batch: envelope validation or proceed to execution.
pub(super) async fn decide_when_tool_calls_present(
    delivery: FormatRetryDelivery<'_>,
    stream: &StreamTx,
    state: &AppState,
    tools: &ToolRegistry,
    history: &mut Vec<ChatMessage>,
    settings: &ModelSettings,
    provider: &OpenAIProvider,
    conversation_id: &str,
    cancel: &CancellationToken,
    tool_budget: &mut SessionToolBudget,
    consumed_single: Option<&mut u32>,
    max_cap: u32,
    scope: &ToolBudgetExhaustionScope,
    final_tool_calls: &[ToolCall],
    log_prefix: &str,
) -> Result<PostAssistantTurnAction> {
    if let Err(err) = crate::tools::validate_envelope_tool_batch(tools, final_tool_calls) {
        log::warn!("{log_prefix} tool envelope batch rejected: {err}");
        let hint = json_tool_envelope_batch_retry_message(&err);
        push_format_retry_user_line(delivery, history, hint);
        sync_out_and_bail_if_exhausted(
            stream,
            state,
            history,
            settings,
            provider,
            conversation_id,
            cancel,
            tool_budget,
            consumed_single,
            max_cap,
            scope,
        )
        .await?;
        return Ok(PostAssistantTurnAction::RetryLoop);
    }
    Ok(PostAssistantTurnAction::ExecuteTools)
}

/// After optional `tool_budget.sync_out`, if the budget is exhausted: emit, compress, cancel, and fail.
pub(super) async fn bail_on_tool_budget_exhausted(
    stream: &StreamTx,
    state: &AppState,
    history: &mut Vec<ChatMessage>,
    settings: &ModelSettings,
    provider: &OpenAIProvider,
    conversation_id: &str,
    cancel: &CancellationToken,
    tool_budget: &mut SessionToolBudget,
    mut consumed_single: Option<&mut u32>,
    max_cap: u32,
    scope: &ToolBudgetExhaustionScope,
) -> Result<()> {
    if !tool_budget.is_exhausted() {
        return Ok(());
    }
    emit(
        stream,
        StreamEvent::ToolRoundsExhausted {
            conversation_id: conversation_id.to_string(),
            max_rounds: max_cap,
            message: scope.user_hint.clone(),
            will_retry_after_compress: settings.context_compression_enabled,
        },
    );
    let _ = crate::context_compression::maybe_compress_after_tool_round_limit(
        history,
        settings,
        provider,
        conversation_id,
        stream,
        cancel.clone(),
        scope.compress_for_session,
    )
    .await;
    if let Some(consumed) = consumed_single.as_mut() {
        tool_budget.sync_out(consumed);
    }
    state.computer_state.mark_cancelled(conversation_id);
    Err(anyhow!(scope.error_message.clone()))
}

pub(super) fn sub_agent_run_result(
    task_id: &str,
    def: &AgentDef,
    content: String,
    reasoning_in_messages: bool,
    reasoning: String,
) -> AgentRunResult {
    AgentRunResult {
        task_id: task_id.to_string(),
        agent_id: def.id.clone(),
        agent_name: def.name.clone(),
        content,
        reasoning: if reasoning_in_messages && !reasoning.is_empty() {
            Some(reasoning)
        } else {
            None
        },
    }
}
