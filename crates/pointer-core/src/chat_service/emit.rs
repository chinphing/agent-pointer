use crate::models::{AgentTrace, StreamEvent};
use serde_json::Value;

use super::StreamTx;

/// Stable trace row id: one row per sub-task (avoids overwriting when the same agent runs twice).
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
    if tx.send(ev).is_err() {
        log::warn!("stream event not delivered (frontend channel closed)");
    }
}

pub(crate) fn emit_agent_step(
    stream: &StreamTx,
    message_id: &str,
    trace: &mut Vec<AgentTrace>,
    agent: AgentTrace,
) {
    if let Some(existing) = trace.iter_mut().find(|item| item.id == agent.id) {
        *existing = agent.clone();
    } else {
        trace.push(agent.clone());
    }
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
    document: Value,
) {
    emit(
        stream,
        StreamEvent::TaskBoardUpdated {
            conversation_id: conversation_id.to_string(),
            store_key: store_key.to_string(),
            document,
        },
    );
}
