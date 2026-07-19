use crate::models::{AgentTrace, StreamEvent};
use crate::stream_broadcast::publish_stream;
use serde_json::Value;

use super::StreamTx;

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
