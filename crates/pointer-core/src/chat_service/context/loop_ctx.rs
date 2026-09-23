//! Agent loop entry contexts (lead single, sub, supervisor).

use crate::agent_instance_scope::AgentInstanceScope;
use crate::agents::{AgentPlan, AgentTask};
use crate::dispatcher::TriggerSource;
use crate::llm_token_stats::{ChatLlmTokenSession, ConversationLlmStats};
use crate::models::{AgentTrace, ChatMessage, ModelSettings};
use crate::provider::OpenAIProvider;
use std::collections::HashMap;

use super::super::session_budget::SessionToolBudget;
use super::super::sub_agent_prompt::SubAgentDefinitionSource;
use super::session::{SessionRefs, SessionRefsArc};

/// Lead single-agent inner loop.
pub struct LeadAgentLoopContext<'a> {
    pub session: SessionRefsArc<'a>,
    pub history: &'a mut Vec<ChatMessage>,
    pub enabled_skill_ids: &'a mut Vec<String>,
    pub agent_skill_overrides: &'a HashMap<String, Vec<String>>,
    pub agent_plan: &'a AgentPlan,
    pub provider: &'a OpenAIProvider,
    pub settings: &'a ModelSettings,
    pub main_task_board_store_key: &'a str,
    pub tool_approval_mode: &'a str,
    pub tool_budget: &'a mut SessionToolBudget,
    pub consumed_single: &'a mut u32,
    pub max_cap: u32,
    pub token_session: &'a mut ChatLlmTokenSession,
    pub reasoning_in_messages: bool,
    pub trigger_source: Option<TriggerSource>,
}

/// Sub-agent run loop.
pub struct SubAgentLoopContext<'a> {
    pub session: SessionRefs<'a>,
    pub provider: &'a OpenAIProvider,
    pub parent_task_board_store_key: &'a str,
    pub message_id: &'a str,
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub enabled_skill_ids: &'a [String],
    pub agent_skill_overrides: &'a HashMap<String, Vec<String>>,
    pub task: &'a AgentTask,
    pub definition_source: SubAgentDefinitionSource<'a>,
    pub instance_scope: AgentInstanceScope,
    pub sub_tool_budget: &'a mut SessionToolBudget,
    pub llm_stats: &'a mut ConversationLlmStats,
    /// Depth of this sub-agent run (lead's first child = 1).
    pub spawn_depth: u32,
    pub max_spawn_depth: u32,
    pub state_arc: std::sync::Arc<crate::chat_service::app_state::AppState>,
    /// Background worker job id for this nested loop.
    pub background_job_id: Option<String>,
    /// Restored transcript when continuing a finished worker (`followupInstanceId`).
    pub resume_history: Option<super::super::worker_followup::ResumedWorkerHistory>,
}

/// Nested `run_subagent` delegation from a tool pass.
pub struct SubagentDelegationContext<'a> {
    pub session: SessionRefs<'a>,
    pub parent_task_board_store_key: &'a str,
    pub message_id: &'a str,
    pub provider: &'a OpenAIProvider,
    pub run_id: &'a str,
    pub allow_agents: &'a [String],
    pub current_agent_id: &'a str,
    pub enabled_skill_ids: &'a [String],
    pub agent_skill_overrides: &'a HashMap<String, Vec<String>>,
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub llm_stats: &'a mut ConversationLlmStats,
    pub tool_call_id: &'a str,
    pub args_value: serde_json::Value,
    /// Depth of the agent issuing `run_subagent` (lead = 0).
    pub parent_spawn_depth: u32,
    /// Sub-agent instance that issued this spawn. `None` is the lead.
    pub parent_agent_instance_id: Option<&'a str>,
    /// Lead transcript buffer; used to persist `agent_trace` on the anchor assistant row.
    pub history: Option<&'a mut Vec<ChatMessage>>,
    pub state_arc: std::sync::Arc<crate::chat_service::app_state::AppState>,
}
