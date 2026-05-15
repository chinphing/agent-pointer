use crate::agents::{AgentDef, AgentTask};
use crate::models::{AgentTrace, StreamEvent};

use super::StreamTx;

pub(crate) fn emit(tx: &StreamTx, ev: StreamEvent) {
    if tx.send(ev).is_err() {
        log::warn!("stream event not delivered (frontend channel closed)");
    }
}

pub(crate) fn emit_agent_content_delta(
    stream: &StreamTx,
    message_id: &str,
    trace: &mut Vec<AgentTrace>,
    def: &AgentDef,
    task: &AgentTask,
    content: String,
    trace_depth: u32,
) {
    emit_agent_step(
        stream,
        message_id,
        trace,
        AgentTrace {
            id: def.id.clone(),
            name: def.name.clone(),
            role: def.role.clone(),
            status: "running".into(),
            detail: Some(if task.title.is_empty() {
                task.instruction.clone()
            } else {
                task.title.clone()
            }),
            content: Some(content),
            depth: Some(trace_depth),
        },
    );
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
