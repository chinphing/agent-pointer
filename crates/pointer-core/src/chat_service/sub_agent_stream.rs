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
use super::provider_stream::{is_recoverable_provider_stream_error, provider_stream_recoverable_retry_message};
use super::util::{new_id, now_ms};
use crate::provider::ProviderEvent;

/// Outcome of one sub-agent provider stream round.
#[derive(Debug)]
pub(super) enum SubAgentStreamOutcome {
    Completed(StreamRoundBuffers),
    /// Recoverable wire error: caller should `continue` the outer tool loop.
    RetryAfterRecoveryHint,
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

    let (tx, mut rx) = mpsc::channel::<ProviderEvent>(64);
    let round_settings = if sub.def.profile == AgentProfile::Computer {
        state
            .computer_state
            .apply_round_settings(conversation_id, &provider.settings)
    } else {
        provider.settings.clone()
    };
    let prov = crate::provider::OpenAIProvider::new(round_settings, provider.api_key.clone());
    let cancel_clone = cancel_owned(&cancel);
    let dump_lbl = format!(
        "{}_{}_sub_{}",
        conversation_id, sub.message_id, sub.task.id
    );
    let handle = tokio::spawn(async move {
        prov.stream_chat(
            &input.history_for_api,
            &input.system_prompts,
            input.native_tools,
            tx,
            cancel_clone,
            Some(dump_lbl.as_str()),
            crate::message_context::LlmHistoryScope::SubAgentLoop,
        )
        .await
    });

    let mut buffers = StreamRoundBuffers::default();
    let mut llm_recorder = LlmRoundRecorder::Scoped {
        stats: sub.llm_stats,
        scope: sub.instance_scope,
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
            if tools_appendix_enabled && is_recoverable_provider_stream_error(&err) {
                log::warn!(
                    "recoverable provider stream error sub_agent task_id={} agent={}: {err:#}",
                    sub.task.id,
                    sub.def.id
                );
                let hint = provider_stream_recoverable_retry_message(
                    &err,
                    effective_max_tokens(&provider.settings),
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
                            will_retry_after_compress: provider.settings.context_compression_enabled,
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
