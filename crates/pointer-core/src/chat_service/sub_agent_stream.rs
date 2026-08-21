//! One sub-agent `stream_chat` round: spawn provider task, drain events, await join outcome.

use crate::agents::agent_ui::agent_display_label;
use crate::agents::AgentProfile;
use crate::models::{effective_max_tokens, ChatMessage, Role, StreamEvent};
use anyhow::{anyhow, Result};
use tokio::sync::mpsc;

use super::agent_stream_round::{
    drain_provider_events, ContentDeltaMode, LlmRoundRecorder, StreamRoundBuffers,
};
use super::context::{cancel_owned, StreamRoundInput, SubStreamRoundContext, SubStreamRoundRefs};
use super::emit::emit;
use super::provider_stream::{
    is_http_429, is_recoverable_provider_stream_error, provider_stream_recoverable_retry_message,
    rate_limit_retry_delay,
};
use super::util::{new_id, now_ms};
use crate::provider::ProviderEvent;

/// Outcome of one sub-agent provider stream round.
#[derive(Debug)]
pub(super) enum SubAgentStreamOutcome {
    Completed(StreamRoundBuffers),
    /// Recoverable wire error: caller should `continue` the outer tool loop.
    RetryAfterRecoveryHint,
    /// Context overflow: local history was pruned/compressed; retry the LLM round.
    RetryAfterOverflowCompress,
}

pub(super) async fn run_sub_agent_stream_round(
    ctx: &mut SubStreamRoundContext<'_>,
    sub: &mut SubStreamRoundRefs<'_>,
    input: StreamRoundInput,
) -> Result<SubAgentStreamOutcome> {
    let stream = ctx.session.stream;
    let state = ctx.session.state;
    let conversation_id = ctx.session.conversation_id;
    let provider = ctx.provider;
    let max_cap = ctx.max_cap;
    let cancel = ctx.cancel.clone();
    let reasoning_in_messages = ctx.reasoning_in_messages;
    let tools_appendix_enabled = input.tools_appendix_enabled;
    let run_id = sub.instance_scope.run_id.clone();
    let trace_bus = ctx.session.state.trace_bus.clone();

    let (tx, mut rx) = mpsc::channel::<ProviderEvent>(64);
    let round_settings = if sub.def.profile == AgentProfile::Computer {
        state
            .computer_state
            .apply_round_settings(conversation_id, &provider.settings)
    } else {
        provider.settings.clone()
    };
    let source = crate::llm_token_stats::active_provider_source(&round_settings);
    let prov = crate::provider::OpenAIProvider::new(round_settings, provider.api_key.clone())
        .with_trace(crate::provider::LlmTraceScope {
            bus: trace_bus.clone(),
            run_id: run_id.clone(),
            conversation_id: conversation_id.to_string(),
            label: "sub".to_string(),
        });
    let cancel_clone = cancel_owned(&cancel);
    let dump_lbl = format!("{}_{}_sub_{}", conversation_id, sub.message_id, sub.task.id);
    // Build wire from local_history + injects before spawn (no full history clone on the HTTP task).
    // Use `prov` (round settings) — not session `provider` — so computer-tier LLM overrides apply.
    let wire = prov.build_stream_chat_wire(
        sub.local_history,
        &input.injected_tail,
        &input.system_prompts,
        input.native_tools,
        Some(dump_lbl.as_str()),
        crate::message_context::LlmHistoryScope::SubAgentLoop,
    )?;
    drop(input.injected_tail);
    drop(input.system_prompts);

    let handle = tokio::spawn(async move {
        prov.stream_chat_wired(wire, tx, cancel_clone, Some(dump_lbl.as_str()))
            .await
    });

    let mut buffers = StreamRoundBuffers::default();
    let mut llm_recorder = LlmRoundRecorder::Scoped {
        stats: sub.llm_stats,
        scope: sub.instance_scope,
        source,
    };
    drain_provider_events(
        &mut rx,
        state,
        sub.message_id,
        reasoning_in_messages,
        ContentDeltaMode::SubAgentTrace {
            trace_id: sub.trace_id.to_string(),
            scoped_message_id: sub.round_message_id.to_string(),
        },
        &mut llm_recorder,
        stream,
        &mut buffers,
        cancel.clone(),
    )
    .await;
    sub.session_content.push_str(&buffers.raw_content_buf);

    match handle.await {
        Ok(Ok(())) => Ok(SubAgentStreamOutcome::Completed(buffers)),
        Ok(Err(err)) => {
            if provider.settings.context_compression_enabled
                && crate::context_compression::is_context_overflow_error(&err)
            {
                log::warn!(
                    "sub_agent: context overflow task_id={} agent={} err={err:#}",
                    sub.task.id,
                    sub.def.id
                );
                crate::context_compression::discard_pending_compression_for_sub_agent(
                    conversation_id,
                    &sub.instance_scope.agent_instance_id,
                );
                emit(
                    stream,
                    StreamEvent::UiToast {
                        conversation_id: conversation_id.to_string(),
                        message: "子任务上下文超限，正在压缩后继续".into(),
                        level: "warning".into(),
                    },
                );
                let recovered = crate::context_compression::recover_history_after_overflow(
                    sub.local_history,
                    &provider.settings,
                    provider,
                    conversation_id,
                    stream,
                    cancel.clone(),
                    crate::context_compression::CompressionUiContext::sub_agent(
                        sub.instance_scope.clone(),
                        sub.message_id,
                        &sub.def.id,
                        &agent_display_label(sub.def),
                        &sub.task.id,
                    ),
                    sub.llm_stats.last_round_prompt_tokens,
                )
                .await;
                if recovered && !cancel.is_cancelled() {
                    return Ok(SubAgentStreamOutcome::RetryAfterOverflowCompress);
                }
                state.computer_state.mark_cancelled(conversation_id);
                return Err(anyhow!(
                    "子任务上下文过大且无法压缩，请新开对话或缩小任务范围"
                ));
            }
            let rate_limit_delay = rate_limit_retry_delay(&err, sub.local_history);
            let retryable = tools_appendix_enabled
                && is_recoverable_provider_stream_error(&err)
                && (!is_http_429(&err) || rate_limit_delay.is_some());
            if retryable {
                log::warn!(
                    "recoverable provider stream error sub_agent task_id={} agent={}: {err:#}",
                    sub.task.id,
                    sub.def.id
                );
                if let Some(delay) = rate_limit_delay.filter(|delay| !delay.is_zero()) {
                    log::info!(
                        "provider rate limit: delaying sub-agent retry task_id={} delay_secs={}",
                        sub.task.id,
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
                        span.conversation_id = conversation_id.to_string();
                        span.attributes = serde_json::json!({ "attempt": 1, "delay_ms": delay.as_millis() as u64 });
                        span.end();
                        trace_bus.emit(span);
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = cancel.cancelled() => return Err(anyhow!("请求已取消")),
                    }
                }
                let hint = provider_stream_recoverable_retry_message(
                    &err,
                    effective_max_tokens(&provider.settings),
                    rate_limit_delay,
                );
                sub.local_history.push(ChatMessage {
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
                    attachments: None,
                    anchor_message_id: None,
                    trace_id: None,
                    task_id: None,
                    spawn_depth: None,
                });
                if ctx.sub_tool_budget.is_exhausted() {
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
                            will_retry_after_compress: provider
                                .settings
                                .context_compression_enabled,
                        },
                    );
                    let _ = crate::context_compression::maybe_compress_after_tool_round_limit(
                        sub.local_history,
                        &provider.settings,
                        provider,
                        conversation_id,
                        stream,
                        cancel.clone(),
                        false,
                        crate::context_compression::CompressionUiContext::sub_agent(
                            sub.instance_scope.clone(),
                            sub.message_id,
                            &sub.def.id,
                            &agent_display_label(sub.def),
                            &sub.task.id,
                        ),
                        sub.llm_stats.last_round_prompt_tokens,
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
