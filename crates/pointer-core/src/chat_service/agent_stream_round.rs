//! Shared `stream_chat` round: drain `ProviderEvent`s and collect assistant output.

use crate::json_tool_caller::JsonToolFinishDiagnostics;
use crate::llm_token_stats::{ChatLlmTokenSession, ConversationLlmStats, LlmUsageSnapshot};
use crate::models::{StreamEvent, ToolCall};
use crate::provider::ProviderEvent;
use crate::tools::parse_tool_call_arguments;
use std::collections::HashSet;

use super::app_state::AppState;
use super::emit::{emit, trace_id_opt};
use super::StreamTx;

/// Collected output from one provider stream round (lead or sub-agent).
#[derive(Debug)]
pub(super) struct StreamRoundBuffers {
    pub raw_content_buf: String,
    pub reasoning_buf: String,
    pub final_tool_calls: Vec<ToolCall>,
    pub finish_reason: String,
    pub json_finish_diag: JsonToolFinishDiagnostics,
    pub xml_thoughts: Option<String>,
    pub xml_headline: Option<String>,
}

impl Default for StreamRoundBuffers {
    fn default() -> Self {
        Self {
            raw_content_buf: String::new(),
            reasoning_buf: String::new(),
            final_tool_calls: Vec::new(),
            finish_reason: String::from("stop"),
            json_finish_diag: JsonToolFinishDiagnostics::default(),
            xml_thoughts: None,
            xml_headline: None,
        }
    }
}

pub(super) enum LlmRoundRecorder<'a> {
    TokenSession {
        session: &'a mut ChatLlmTokenSession,
        model_name: Option<&'a str>,
    },
    Scoped {
        stats: &'a mut ConversationLlmStats,
        scope: &'a crate::agent_instance_scope::AgentInstanceScope,
        model_name: Option<&'a str>,
    },
}

impl LlmRoundRecorder<'_> {
    fn record(&mut self, usage: Option<&LlmUsageSnapshot>) {
        match self {
            LlmRoundRecorder::TokenSession { session, model_name } => {
                session.stats.record_llm_round(
                    &session.lead_scope,
                    usage,
                    *model_name,
                );
            }
            LlmRoundRecorder::Scoped {
                stats,
                scope,
                model_name,
            } => stats.record_llm_round(scope, usage, *model_name),
        }
    }
}

/// How to emit streaming assistant text to the UI.
pub(super) enum ContentDeltaMode<'a> {
    /// Lead single-agent turn: raw + visible deltas on `assistant_id`.
    LeadMessage {
        stream: &'a StreamTx,
        message_id: String,
    },
    /// Sub-agent under supervisor / `run_subagent`: UI routed by `trace_id` (no trace.content streaming).
    SubAgentTrace {
        trace_id: String,
    },
}

pub(super) async fn drain_provider_events(
    rx: &mut tokio::sync::mpsc::Receiver<ProviderEvent>,
    state: &AppState,
    message_id: &str,
    reasoning_in_messages: bool,
    content_mode: ContentDeltaMode<'_>,
    llm_recorder: &mut LlmRoundRecorder<'_>,
    stream: &StreamTx,
    buffers: &mut StreamRoundBuffers,
) {
    let sub_trace_id = match &content_mode {
        ContentDeltaMode::LeadMessage { .. } => None,
        ContentDeltaMode::SubAgentTrace { trace_id } => Some(trace_id.as_str()),
    };
    let mut streamed_tool_call_ids: HashSet<String> = HashSet::new();

    while let Some(ev) = rx.recv().await {
        match ev {
            ProviderEvent::ContentDelta(delta) => {
                buffers.raw_content_buf.push_str(&delta);
                match &content_mode {
                    ContentDeltaMode::LeadMessage { stream, message_id } => {
                        emit(
                            stream,
                            StreamEvent::RawContentDelta {
                                message_id: message_id.clone(),
                                text: delta.clone(),
                                trace_id: None,
                            },
                        );
                        emit(
                            stream,
                            StreamEvent::Delta {
                                message_id: message_id.clone(),
                                text: delta,
                            },
                        );
                    }
                    ContentDeltaMode::SubAgentTrace { trace_id } => {
                        emit(
                            stream,
                            StreamEvent::RawContentDelta {
                                message_id: message_id.to_string(),
                                text: delta,
                                trace_id: trace_id_opt(Some(trace_id.as_str())),
                            },
                        );
                    }
                }
            }
            ProviderEvent::ReasoningDelta(delta) => {
                if reasoning_in_messages {
                    buffers.reasoning_buf.push_str(&delta);
                }
                emit(
                    stream,
                    StreamEvent::ReasoningDelta {
                        message_id: message_id.to_string(),
                        text: delta,
                        trace_id: trace_id_opt(sub_trace_id),
                    },
                );
            }
            ProviderEvent::ToolCallStart { id, name, .. } => {
                emit(
                    stream,
                    StreamEvent::ToolCallStart {
                        message_id: message_id.to_string(),
                        tool_call: ToolCall {
                            id,
                            name: name.clone(),
                            arguments: String::new(),
                            status: "pending".into(),
                            result: None,
                            error: None,
                            duration_ms: None,
                            risk_level: state
                                .tools
                                .tool_risk_level_for_invocation(
                                    &name,
                                    &parse_tool_call_arguments(""),
                                )
                                .or(Some("low".into())),
                            display_label: None,
                            display_summary: None,
                        },
                        trace_id: trace_id_opt(sub_trace_id),
                    },
                );
            }
            ProviderEvent::ToolCallArgsDelta {
                tool_call_id, args, ..
            } => {
                emit(
                    stream,
                    StreamEvent::ToolCallArgsDelta {
                        message_id: message_id.to_string(),
                        tool_call_id,
                        args_delta: args,
                        trace_id: trace_id_opt(sub_trace_id),
                    },
                );
            }
            ProviderEvent::JsonToolStreamingReady { tool_calls, .. } => {
                emit_deduped_tool_starts(
                    stream,
                    message_id,
                    state,
                    &tool_calls,
                    &mut streamed_tool_call_ids,
                    sub_trace_id,
                );
            }
            ProviderEvent::AssistantJsonPartial {
                thoughts,
                headline,
                tool_name,
                response_text,
            } => {
                emit(
                    stream,
                    StreamEvent::AssistantJsonPartial {
                        message_id: message_id.to_string(),
                        thoughts,
                        headline,
                        tool_name,
                        response_text,
                        trace_id: trace_id_opt(sub_trace_id),
                    },
                );
            }
            ProviderEvent::Finish {
                reason,
                tool_calls,
                json,
                thoughts,
                headline,
                usage,
            } => {
                buffers.finish_reason = reason;
                buffers.json_finish_diag = json;
                buffers.xml_thoughts = thoughts;
                buffers.xml_headline = headline;
                llm_recorder.record(usage.as_ref());
                emit_deduped_tool_starts(
                    stream,
                    message_id,
                    state,
                    &tool_calls,
                    &mut streamed_tool_call_ids,
                    sub_trace_id,
                );
                buffers.final_tool_calls = tool_calls;
            }
        }
    }
}

fn emit_deduped_tool_starts(
    stream: &StreamTx,
    message_id: &str,
    state: &AppState,
    tool_calls: &[ToolCall],
    streamed_ids: &mut HashSet<String>,
    trace_id: Option<&str>,
) {
    for tc in tool_calls {
        if streamed_ids.insert(tc.id.clone()) {
            let mut t = tc.clone();
            let args_v = parse_tool_call_arguments(&t.arguments);
            t.risk_level = state
                .tools
                .tool_risk_level_for_invocation(&t.name, &args_v)
                .or(Some("low".into()));
            emit(
                stream,
                StreamEvent::ToolCallStart {
                    message_id: message_id.to_string(),
                    tool_call: t,
                    trace_id: trace_id_opt(trace_id),
                },
            );
        }
    }
}
