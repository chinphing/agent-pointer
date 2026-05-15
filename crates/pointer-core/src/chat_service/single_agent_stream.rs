//! One `stream_chat` round: spawn provider task, drain `ProviderEvent`s, await join outcome.

use crate::json_tool_caller::JsonToolFinishDiagnostics;
use crate::llm_token_stats::ChatLlmTokenSession;
use crate::models::{effective_max_tokens, ChatMessage, ModelSettings, StreamEvent, ToolCall};
use crate::provider::OpenAIProvider;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::agent_stream_round::{
    drain_provider_events, ContentDeltaMode, LlmRoundRecorder, StreamRoundBuffers,
};
use super::app_state::AppState;
use super::emit::emit;
use super::json_tool_retries::push_injected_format_retry_turn;
use super::provider_stream::{is_recoverable_provider_stream_error, provider_stream_recoverable_retry_message};
use super::session_budget::SessionToolBudget;
use super::StreamTx;

/// Collected assistant output after a successful provider stream.
#[derive(Debug)]
pub(super) struct SingleAgentRoundStream {
    pub raw_content_buf: String,
    pub reasoning_buf: String,
    pub final_tool_calls: Vec<ToolCall>,
    pub finish_reason: String,
    pub json_finish_diag: JsonToolFinishDiagnostics,
    pub xml_thoughts: Option<String>,
    pub xml_headline: Option<String>,
}

/// Outcome of spawning and draining one `stream_chat` round.
#[derive(Debug)]
pub(super) enum ProviderRoundOutcome {
    Completed(SingleAgentRoundStream),
    /// Recoverable wire error: caller should `continue` the outer tool loop.
    RetryAfterRecoveryHint,
}

pub(super) async fn run_provider_stream_round(
    stream: StreamTx,
    state: Arc<AppState>,
    provider: &OpenAIProvider,
    settings: &ModelSettings,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    llm_token_session: &mut ChatLlmTokenSession,
    assistant_id: String,
    tools_appendix_enabled: bool,
    tool_budget: &mut SessionToolBudget,
    consumed_single: &mut u32,
    max_cap: u32,
    cancel: CancellationToken,
    reasoning_in_messages: bool,
    history_for_api: Vec<ChatMessage>,
    prompts_with_env: Vec<String>,
) -> Result<ProviderRoundOutcome> {
    let (tx, mut rx) = mpsc::channel(64);
    let prov = OpenAIProvider::new(provider.settings.clone(), provider.api_key.clone());
    let prompts_clone = prompts_with_env;
    let cancel_clone = cancel.clone();
    let dump_lbl = format!("{}_{}", conversation_id, assistant_id);
    let send_handle = tokio::spawn(async move {
        prov.stream_chat(
            &history_for_api,
            &prompts_clone,
            tx,
            cancel_clone,
            Some(dump_lbl.as_str()),
        )
        .await
    });

    let mut buffers = StreamRoundBuffers::default();
    let mut llm_recorder = LlmRoundRecorder::TokenSession(llm_token_session);
    drain_provider_events(
        &mut rx,
        &state,
        &assistant_id,
        reasoning_in_messages,
        ContentDeltaMode::LeadMessage {
            stream: &stream,
            message_id: assistant_id.clone(),
        },
        &mut llm_recorder,
        &stream,
        &mut buffers,
    )
    .await;

    match send_handle.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            if tools_appendix_enabled && is_recoverable_provider_stream_error(&e) {
                log::warn!(
                    "recoverable provider stream error conversation_id={} assistant_id={}: {e:#}",
                    conversation_id,
                    assistant_id
                );
                emit(
                    &stream,
                    StreamEvent::MessageEnd {
                        message_id: assistant_id.clone(),
                        content: None,
                        raw_content: None,
                        thoughts: None,
                        headline: None,
                    },
                );
                let hint =
                    provider_stream_recoverable_retry_message(&e, effective_max_tokens(settings));
                push_injected_format_retry_turn(&stream, conversation_id, history, hint);
                tool_budget.sync_out(consumed_single);
                if tool_budget.is_exhausted() {
                    let hint = format!(
                        "单智能体模式下工具调用累计已达上限（{} 轮，含此前消息）。建议新开对话；将尝试压缩上下文以便查看摘要。",
                        max_cap
                    );
                    emit(
                        &stream,
                        StreamEvent::ToolRoundsExhausted {
                            conversation_id: conversation_id.to_string(),
                            max_rounds: max_cap,
                            message: hint,
                            will_retry_after_compress: settings.context_compression_enabled,
                        },
                    );
                    let _ = crate::context_compression::maybe_compress_after_tool_round_limit(
                        history,
                        settings,
                        provider,
                        conversation_id,
                        &stream,
                        cancel.clone(),
                        true,
                    )
                    .await;
                    tool_budget.sync_out(consumed_single);
                    state.computer_state.mark_cancelled(conversation_id);
                    return Err(anyhow!(
                        "单智能体模式下工具调用轮次已达上限（{max_cap}）。请新开对话或在设置中调高上限。"
                    ));
                }
                return Ok(ProviderRoundOutcome::RetryAfterRecoveryHint);
            }
            emit(
                &stream,
                StreamEvent::Error {
                    message_id: Some(assistant_id.clone()),
                    message: e.to_string(),
                },
            );
            emit(
                &stream,
                StreamEvent::MessageEnd {
                    message_id: assistant_id.clone(),
                    content: None,
                    raw_content: None,
                    thoughts: None,
                    headline: None,
                },
            );
            tool_budget.sync_out(consumed_single);
            state.computer_state.mark_cancelled(conversation_id);
            return Err(e);
        }
        Err(e) => {
            tool_budget.sync_out(consumed_single);
            state.computer_state.mark_cancelled(conversation_id);
            return Err(anyhow!("任务异常：{e}"));
        }
    }

    Ok(ProviderRoundOutcome::Completed(SingleAgentRoundStream {
        raw_content_buf: buffers.raw_content_buf,
        reasoning_buf: buffers.reasoning_buf,
        final_tool_calls: buffers.final_tool_calls,
        finish_reason: buffers.finish_reason,
        json_finish_diag: buffers.json_finish_diag,
        xml_thoughts: buffers.xml_thoughts,
        xml_headline: buffers.xml_headline,
    }))
}
