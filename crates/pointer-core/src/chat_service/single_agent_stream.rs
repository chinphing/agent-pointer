//! One `stream_chat` round: spawn provider task, drain `ProviderEvent`s, await join outcome.

use crate::models::{effective_max_tokens, StreamEvent, ToolCall};
use anyhow::Result;
use tokio::sync::mpsc;

use super::agent_stream_round::{
    drain_provider_events, ContentDeltaMode, LlmRoundRecorder, StreamRoundBuffers,
};
use super::context::{cancel_owned, LeadStreamRoundContext, StreamRoundInput};
use super::emit::emit;
use super::json_tool_retries::push_injected_format_retry_turn;
use super::provider_stream::{
    is_http_429, is_recoverable_provider_stream_error, provider_stream_recoverable_retry_message,
    rate_limit_retry_delay,
};

/// Collected assistant output after a successful provider stream.
#[derive(Debug)]
pub(super) struct SingleAgentRoundStream {
    pub raw_content_buf: String,
    pub reasoning_buf: String,
    pub final_tool_calls: Vec<ToolCall>,
    pub xml_thoughts: Option<String>,
    pub finish_reason: String,
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
    let run_id = ctx.token_session.run_id.clone();
    let trace_bus = ctx.session.state.trace_bus.clone();

    let (tx, mut rx) = mpsc::channel(64);
    // `settings` is per-round (e.g. computer tier `computerTierLlm`); do not use session `provider.settings`.
    let prov = crate::provider::OpenAIProvider::new(settings.clone(), provider.api_key.clone())
        .with_trace(crate::provider::LlmTraceScope {
            bus: trace_bus.clone(),
            run_id: run_id.clone(),
            conversation_id: conversation_id.clone(),
            label: "lead".to_string(),
        });
    let cancel_clone = cancel_owned(&cancel);
    let dump_lbl = format!("{}_{}", conversation_id, assistant_id);
    // Build wire synchronously from session history + ephemeral injects so the spawned
    // HTTP task does not retain a full `ChatMessage` history clone for the stream lifetime.
    // Use `prov` (round settings) so computer-tier LLM overrides apply to wire + stream.
    let wire = prov.build_stream_chat_wire(
        ctx.history,
        &input.injected_tail,
        &input.system_prompts,
        input.native_tools,
        Some(dump_lbl.as_str()),
        crate::message_context::LlmHistoryScope::Lead,
    )?;
    drop(input.injected_tail);
    drop(input.system_prompts);

    let send_handle = tokio::spawn(async move {
        prov.stream_chat_wired(wire, tx, cancel_clone, Some(dump_lbl.as_str()))
            .await
    });

    let mut buffers = StreamRoundBuffers::default();
    let mut llm_recorder = LlmRoundRecorder::TokenSession {
        session: ctx.token_session,
        source: crate::llm_token_stats::active_provider_source(settings),
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
            let rate_limit_delay = rate_limit_retry_delay(&e, ctx.history);
            let retryable = tools_appendix_enabled
                && is_recoverable_provider_stream_error(&e)
                && (!is_http_429(&e) || rate_limit_delay.is_some());
            if retryable {
                log::warn!(
                    "recoverable provider stream error conversation_id={} assistant_id={}: {e:#}",
                    conversation_id,
                    assistant_id
                );
                if let Some(delay) = rate_limit_delay.filter(|delay| !delay.is_zero()) {
                    log::info!(
                        "provider rate limit: delaying retry conversation_id={} delay_secs={}",
                        conversation_id,
                        delay.as_secs()
                    );
                    {
                        let mut span = crate::observability::TraceEvent::new(
                            run_id.clone(),
                            uuid::Uuid::new_v4().to_string(),
                            crate::observability::SpanKind::Retry,
                            "rate_limit",
                        );
                        span.parent_span_id = Some("run-root".to_string());
                        span.run_id = run_id.clone();
                        span.conversation_id = conversation_id.clone();
                        span.attributes = serde_json::json!({ "attempt": 1, "delay_ms": delay.as_millis() as u64 });
                        span.end();
                        trace_bus.emit(span);
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = cancel.cancelled() => {
                            return Err(super::emit::chat_run_err(
                                "请求已取消",
                                Some(assistant_id.clone()),
                            ));
                        }
                    }
                }
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
                let hint = provider_stream_recoverable_retry_message(
                    &e,
                    effective_max_tokens(settings),
                    rate_limit_delay,
                );
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
                        ctx.token_session.stats.last_round_prompt_tokens,
                    )
                    .await;
                    ctx.tool_budget.sync_out(ctx.consumed_single);
                    state.computer_state.mark_cancelled(&conversation_id);
                    return Err(super::emit::chat_run_err(
                        format!(
                            "单智能体模式下工具调用轮次已达上限（{max_cap}）。请新开对话或在设置中调高上限。"
                        ),
                        Some(assistant_id.clone()),
                    ));
                }
                return Ok(ProviderRoundOutcome::RetryAfterRecoveryHint);
            }
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
            // UI Error is emitted once from run_chat with this message_id.
            return Err(super::emit::chat_run_err(
                e.to_string(),
                Some(assistant_id.clone()),
            ));
        }
        Err(e) => {
            ctx.tool_budget.sync_out(ctx.consumed_single);
            state.computer_state.mark_cancelled(&conversation_id);
            return Err(super::emit::chat_run_err(
                format!("任务异常：{e}"),
                Some(assistant_id.clone()),
            ));
        }
    }

    Ok(ProviderRoundOutcome::Completed(SingleAgentRoundStream {
        raw_content_buf: buffers.raw_content_buf,
        reasoning_buf: buffers.reasoning_buf,
        final_tool_calls: buffers.final_tool_calls,
        xml_thoughts: buffers.xml_thoughts,
        finish_reason: buffers.finish_reason,
    }))
}
