//! Per agent launch identity for token reporting and structured logs.

use uuid::Uuid;

/// One runtime agent invocation (lead, sub-agent, or supervisor segment).
#[derive(Debug, Clone)]
pub struct AgentInstanceScope {
    pub agent_instance_id: String,
    pub agent_role_id: String,
    pub conversation_id: String,
}

impl AgentInstanceScope {
    pub fn new(conversation_id: impl Into<String>, agent_role_id: impl Into<String>) -> Self {
        Self {
            agent_instance_id: Uuid::new_v4().to_string(),
            agent_role_id: agent_role_id.into(),
            conversation_id: conversation_id.into(),
        }
    }

    pub fn log_suffix(&self) -> String {
        format!(
            "conversation_id={} agent_instance_id={} agent_role_id={}",
            self.conversation_id, self.agent_instance_id, self.agent_role_id
        )
    }
}
