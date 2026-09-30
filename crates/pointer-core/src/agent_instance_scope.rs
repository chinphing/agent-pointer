//! Per agent launch identity for token reporting and structured logs.

use uuid::Uuid;

/// One runtime agent invocation (lead or sub-agent).
#[derive(Debug, Clone)]
pub struct AgentInstanceScope {
    pub run_id: String,
    pub agent_instance_id: String,
    pub agent_role_id: String,
    pub conversation_id: String,
}

impl AgentInstanceScope {
    pub fn new(
        run_id: impl Into<String>,
        conversation_id: impl Into<String>,
        agent_role_id: impl Into<String>,
    ) -> Self {
        Self::with_instance_id(
            run_id,
            conversation_id,
            agent_role_id,
            Uuid::new_v4().to_string(),
        )
    }

    pub fn with_instance_id(
        run_id: impl Into<String>,
        conversation_id: impl Into<String>,
        agent_role_id: impl Into<String>,
        agent_instance_id: impl Into<String>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            agent_instance_id: agent_instance_id.into(),
            agent_role_id: agent_role_id.into(),
            conversation_id: conversation_id.into(),
        }
    }

    pub fn log_suffix(&self) -> String {
        format!(
            "run_id={} conversation_id={} agent_instance_id={} agent_role_id={}",
            self.run_id, self.conversation_id, self.agent_instance_id, self.agent_role_id
        )
    }
}

pub fn agent_instance_id_system_line(agent_instance_id: &str) -> String {
    format!("Your agentInstanceId is {agent_instance_id}.")
}
