//! One sub-agent `stream_chat` round: spawn provider task, drain events, await join outcome.

use crate::agents::agent_ui::agent_display_label;
use crate::agents::{AgentDef, AgentProfile, AgentTask};
use crate::llm_token_stats::ConversationLlmStats;
use crate::models::{effective_max_tokens, ChatMessage, StreamEvent, SystemPromptSections};
use crate::provider::{OpenAIProvider, ProviderEvent};
use anyhow::{anyhow, Result};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::agent_stream_round::{
    drain_provider_events, ContentDeltaMode, LlmRoundRecorder, StreamRoundBuffers,
};
use super::app_state::AppState;
use super::emit::{agent_trace_step_id, emit};
use super::provider_stream::{is_recoverable_provider_stream_error, provider_stream_recoverable_retry_message};
use super::session_budget::SessionToolBudget;
use super::util::{new_id, now_ms};
use super::StreamTx;
use crate::models::{AgentTrace, Role};

/// Outcome of one sub-agent provider stream round.
#[derive(Debug)]
pub(super) enum SubAgentStreamOutcome {
    Completed(StreamRoundBuffers),
    /// Recoverable wire error: caller should `continue` the outer tool loop.
    RetryAfterRecoveryHint,
}

pub(super) async fn run_sub_agent_stream_round(
    stream: &StreamTx,
    state: &AppState,
    provider: &OpenAIProvider,
    conversation_id: &str,
    message_id: &str,
    task: &AgentTask,
    def: &AgentDef,
    instance_scope: &crate::agent_instance_scope::AgentInstanceScope,
    _agent_trace: &mut Vec<AgentTrace>,
    session_content: &mut String,
    reasoning_in_messages: bool,
    llm_stats: &mut ConversationLlmStats,
    local_history: &mut Vec<ChatMessage>,
    sub_tool_budget: &mut SessionToolBudget,
    max_cap: u32,
    tools_appendix_enabled: bool,
    native_tools: Vec<serde_json::Value>,
    cancel: CancellationToken,
    history_for_api: Vec<ChatMessage>,
    system_prompts: SystemPromptSections,
) -> Result<SubAgentStreamOutcome> {
    let (tx, mut rx) = mpsc::channel::<ProviderEvent>(64);
    let round_settings = if def.profile == AgentProfile::Computer {
        state
            .computer_state
            .apply_round_settings(conversation_id, &provider.settings)
    } else {
        provider.settings.clone()
    };
    let prov = OpenAIProvider::new(round_settings, provider.api_key.clone());
    let cancel_clone = cancel.clone();
    let dump_lbl = format!("{}_{}_sub_{}", conversation_id, message_id, task.id);
    let system_clone = system_prompts;
    let handle = tokio::spawn(async move {
        prov.stream_chat(
            &history_for_api,
            &system_clone,
            native_tools,
            tx,
            cancel_clone,
            Some(dump_lbl.as_str()),
        )
        .await
    });

    let trace_id = agent_trace_step_id(&task.id, &def.id);
    let mut buffers = StreamRoundBuffers::default();
    let model_name = if provider.settings.model.trim().is_empty() {
        None
    } else {
        Some(provider.settings.model.as_str())
    };
    let mut llm_recorder = LlmRoundRecorder::Scoped {
        stats: llm_stats,
        scope: instance_scope,
        model_name,
    };
    drain_provider_events(
        &mut rx,
        state,
        message_id,
        reasoning_in_messages,
        ContentDeltaMode::SubAgentTrace {
            trace_id: trace_id.clone(),
        },
        &mut llm_recorder,
        stream,
        &mut buffers,
        cancel.clone(),
    )
    .await;
    session_content.push_str(&buffers.raw_content_buf);

    match handle.await {
        Ok(Ok(())) => Ok(SubAgentStreamOutcome::Completed(buffers)),
        Ok(Err(err)) => {
            if tools_appendix_enabled && is_recoverable_provider_stream_error(&err) {
                log::warn!(
                    "recoverable provider stream error sub_agent task_id={} agent={}: {err:#}",
                    task.id,
                    def.id
                );
                let hint = provider_stream_recoverable_retry_message(
                    &err,
                    effective_max_tokens(&provider.settings),
                );
                local_history.push(ChatMessage {
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
            });
                if sub_tool_budget.is_exhausted() {
                    let hint = format!(
                        "子 Agent 内工具调用累计已达上限（{} 轮）。建议新开对话。",
                        max_cap
                    );
                    emit(
                        stream,
                        StreamEvent::ToolRoundsExhausted {
                            conversation_id: conversation_id.to_string(),
                            max_rounds: max_cap,
                            message: hint,
                            will_retry_after_compress: provider.settings.context_compression_enabled,
                        },
                    );
                    let _ = crate::context_compression::maybe_compress_after_tool_round_limit(
                        local_history,
                        &provider.settings,
                        provider,
                        conversation_id,
                        stream,
                        cancel.clone(),
                        false,
                        crate::context_compression::CompressionUiContext::sub_agent(
                            instance_scope.clone(),
                            message_id,
                            &def.id,
                            &agent_display_label(def),
                            &task.id,
                        ),
                    )
                    .await;
                    state.computer_state.mark_cancelled(conversation_id);
                    return Err(anyhow!(
                        "子 Agent 内工具调用轮次已达上限（{max_cap}）。请新开对话。"
                    ));
                }
                return Ok(SubAgentStreamOutcome::RetryAfterRecoveryHint);
            }
            state.computer_state.mark_cancelled(conversation_id);
            Err(err)
        }
        Err(err) => {
            state.computer_state.mark_cancelled(conversation_id);
            Err(anyhow!("子 Agent 任务异常：{err}"))
        }
    }
}
