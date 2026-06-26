//! One `stream_chat` round: spawn provider task, drain `ProviderEvent`s, await join outcome.

use crate::models::{effective_max_tokens, StreamEvent, ToolCall};
use anyhow::{anyhow, Result};
use tokio::sync::mpsc;

use super::agent_stream_round::{
    drain_provider_events, ContentDeltaMode, LlmRoundRecorder, StreamRoundBuffers,
};
use super::context::{cancel_owned, LeadStreamRoundContext, StreamRoundInput};
use super::emit::emit;
use super::json_tool_retries::push_injected_format_retry_turn;
use super::provider_stream::{is_recoverable_provider_stream_error, provider_stream_recoverable_retry_message};

/// Collected assistant output after a successful provider stream.
#[derive(Debug)]
pub(super) struct SingleAgentRoundStream {
    pub raw_content_buf: String,
    pub reasoning_buf: String,
    pub final_tool_calls: Vec<ToolCall>,
    pub xml_thoughts: Option<String>,
}

/// Outcome of spawning and draining one `stream_chat` round.
#[derive(Debug)]
pub(super) enum ProviderRoundOutcome {
    Completed(SingleAgentRoundStream),
    /// Recoverable wire error: caller should `continue` the outer tool loop.
    RetryAfterRecoveryHint,
}

pub(super) async fn run_provider_stream_round(
    ctx: &mut LeadStreamRoundContext<'_>,
    input: StreamRoundInput,
    assistant_id: String,
) -> Result<ProviderRoundOutcome> {
    let stream = ctx.stream().clone();
    let conversation_id = ctx.conversation_id().to_string();
    let state = ctx.session.state.clone();
    let settings = ctx.settings;
    let provider = ctx.provider;
    let max_cap = ctx.max_cap;
    let cancel = ctx.cancel.clone();
    let reasoning_in_messages = ctx.reasoning_in_messages;
    let tools_appendix_enabled = input.tools_appendix_enabled;

    let (tx, mut rx) = mpsc::channel(64);
    // `settings` is per-round (e.g. computer tier `computerTierLlm`); do not use session `provider.settings`.
    let prov = crate::provider::OpenAIProvider::new(settings.clone(), provider.api_key.clone());
    let cancel_clone = cancel_owned(&cancel);
    let dump_lbl = format!("{}_{}", conversation_id, assistant_id);
    let send_handle = tokio::spawn(async move {
        prov.stream_chat(
            &input.history_for_api,
            &input.system_prompts,
            input.native_tools,
            tx,
            cancel_clone,
            Some(dump_lbl.as_str()),
            crate::message_context::LlmHistoryScope::Lead,
        )
        .await
    });

    let mut buffers = StreamRoundBuffers::default();
    let mut llm_recorder = LlmRoundRecorder::TokenSession {
        session: ctx.token_session,
    };
    drain_provider_events(
        &mut rx,
        state.as_ref(),
        &assistant_id,
        reasoning_in_messages,
        ContentDeltaMode::LeadMessage {
            stream: &stream,
            message_id: assistant_id.clone(),
        },
        &mut llm_recorder,
        &stream,
        &mut buffers,
        cancel.clone(),
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
                        tool_raw_output: None,
                        thoughts: None,
                        headline: None,
                        trace_id: None,
                        scoped_message_id: None,
                        attachments: None,
                    },
                );
                let hint =
                    provider_stream_recoverable_retry_message(&e, effective_max_tokens(settings));
                push_injected_format_retry_turn(&stream, &conversation_id, ctx.history, hint);
                ctx.tool_budget.sync_out(ctx.consumed_single);
                if ctx.tool_budget.is_exhausted() {
                    let hint = format!(
                        "单智能体模式下工具调用累计已达上限（{} 轮，含此前消息）。建议新开对话；将尝试压缩上下文以便查看摘要。",
                        max_cap
                    );
                    emit(
                        &stream,
                        StreamEvent::ToolRoundsExhausted {
                            conversation_id: conversation_id.clone(),
                            max_rounds: max_cap,
                            message: hint,
                            will_retry_after_compress: settings.context_compression_enabled,
                        },
                    );
                    let _ = crate::context_compression::maybe_compress_after_tool_round_limit(
                        ctx.history,
                        settings,
                        provider,
                        &conversation_id,
                        &stream,
                        cancel.clone(),
                        true,
                        crate::context_compression::CompressionUiContext::main(
                            ctx.token_session.lead_scope.clone(),
                        ),
                    )
                    .await;
                    ctx.tool_budget.sync_out(ctx.consumed_single);
                    state.computer_state.mark_cancelled(&conversation_id);
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
                    tool_raw_output: None,
                    thoughts: None,
                    headline: None,
                    trace_id: None,
                    scoped_message_id: None,
                    attachments: None,
                },
            );
            ctx.tool_budget.sync_out(ctx.consumed_single);
            state.computer_state.mark_cancelled(&conversation_id);
            return Err(e);
        }
        Err(e) => {
            ctx.tool_budget.sync_out(ctx.consumed_single);
            state.computer_state.mark_cancelled(&conversation_id);
            return Err(anyhow!("任务异常：{e}"));
        }
    }

    Ok(ProviderRoundOutcome::Completed(SingleAgentRoundStream {
        raw_content_buf: buffers.raw_content_buf,
        reasoning_buf: buffers.reasoning_buf,
        final_tool_calls: buffers.final_tool_calls,
        xml_thoughts: buffers.xml_thoughts,
    }))
}
