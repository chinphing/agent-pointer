use crate::models::{AgentTrace, StreamEvent};
use crate::stream_broadcast::publish_stream;
use serde_json::Value;
use std::fmt;

use super::StreamTx;

/// Failure intended for a single `StreamEvent::Error` from [`super::session::run_chat`].
///
/// Lower layers must **not** emit `StreamEvent::Error` themselves — return this (or a
/// plain `anyhow::Error`) so the session exit path surfaces the UI once.
#[derive(Debug)]
pub(crate) struct ChatRunError {
    pub message: String,
    /// When set, the frontend attaches the error to this assistant bubble instead of
    /// inserting a second empty error row.
    pub message_id: Option<String>,
}

impl fmt::Display for ChatRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ChatRunError {}

/// Prefer this over emitting `StreamEvent::Error` in stream / tool loops.
pub(crate) fn chat_run_err(
    message: impl Into<String>,
    message_id: Option<String>,
) -> anyhow::Error {
    anyhow::Error::new(ChatRunError {
        message: message.into(),
        message_id,
    })
}

pub(crate) fn chat_run_error_parts(err: &anyhow::Error) -> (String, Option<String>) {
    for c in err.chain() {
        if let Some(e) = c.downcast_ref::<ChatRunError>() {
            return (e.message.clone(), e.message_id.clone());
        }
    }
    (err.to_string(), None)
}

/// Legacy registered-agent trace row id (`{taskId}:{agentId}`).
pub(crate) fn agent_trace_step_id(task_id: &str, agent_id: &str) -> String {
    let t = task_id.trim();
    if t.is_empty() {
        agent_id.trim().to_string()
    } else {
        format!("{}:{}", t, agent_id.trim())
    }
}

pub(crate) fn trace_id_opt(id: Option<&str>) -> Option<String> {
    id.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

pub(crate) fn emit(tx: &StreamTx, ev: StreamEvent) {
    publish_stream(tx, ev);
}

/// Close an empty streaming assistant shell before the tool loop starts a new
/// `MessageStart`. Overflow recovery used to skip this, which left a frozen
/// 「思考中.」 bubble in the transcript.
pub(crate) fn emit_empty_assistant_end(
    tx: &StreamTx,
    message_id: impl Into<String>,
    trace_id: Option<String>,
    scoped_message_id: Option<String>,
) {
    emit(
        tx,
        StreamEvent::MessageEnd {
            message_id: message_id.into(),
            content: None,
            raw_content: None,
            tool_raw_output: None,
            thoughts: None,
            headline: None,
            trace_id,
            scoped_message_id,
            attachments: None,
        },
    );
}

pub(crate) fn emit_agent_step(
    stream: &StreamTx,
    message_id: &str,
    trace: &mut Vec<AgentTrace>,
    agent: AgentTrace,
) {
    merge_agent_trace(trace, agent.clone());
    publish_agent_step(stream, message_id, agent);
}

pub(crate) fn merge_agent_trace(trace: &mut Vec<AgentTrace>, agent: AgentTrace) {
    if let Some(existing) = trace.iter_mut().find(|item| item.id == agent.id) {
        *existing = agent;
    } else {
        trace.push(agent);
    }
}

pub(crate) fn publish_agent_step(stream: &StreamTx, message_id: &str, agent: AgentTrace) {
    emit(
        stream,
        StreamEvent::AgentStep {
            message_id: message_id.to_string(),
            agent,
        },
    );
}

pub(crate) fn emit_task_board_updated(
    stream: &StreamTx,
    conversation_id: &str,
    store_key: &str,
    anchor_message_id: Option<String>,
    document: Value,
) {
    emit(
        stream,
        StreamEvent::TaskBoardUpdated {
            conversation_id: conversation_id.to_string(),
            store_key: store_key.to_string(),
            anchor_message_id,
            document,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_run_err_carries_message_id_for_session_emit() {
        let err = chat_run_err("HTTP 400 Bad Request: tool choice", Some("asst-1".into()));
        let (msg, id) = chat_run_error_parts(&err);
        assert_eq!(msg, "HTTP 400 Bad Request: tool choice");
        assert_eq!(id.as_deref(), Some("asst-1"));
        assert_eq!(err.to_string(), "HTTP 400 Bad Request: tool choice");

        let plain = anyhow::anyhow!("plain");
        let (msg2, id2) = chat_run_error_parts(&plain);
        assert_eq!(msg2, "plain");
        assert!(id2.is_none());
    }
}
