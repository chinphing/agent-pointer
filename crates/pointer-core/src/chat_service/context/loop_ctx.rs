//! Agent loop entry contexts (lead single, sub, supervisor).

use crate::agents::{AgentPlan, AgentTask};
use crate::llm_token_stats::{ChatLlmTokenSession, ConversationLlmStats};
use crate::models::{AgentTrace, ChatMessage, ModelSettings};
use crate::provider::OpenAIProvider;

use super::session::{SessionRefs, SessionRefsArc};
use super::super::session_budget::SessionToolBudget;

/// Lead single-agent inner loop.
pub struct LeadAgentLoopContext<'a> {
    pub session: SessionRefsArc<'a>,
    pub history: &'a mut Vec<ChatMessage>,
    pub enabled_skill_ids: &'a mut Vec<String>,
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
    pub planner_outcome: crate::task_board::PlannerRunOutcome,
}

/// Sub-agent run loop.
pub struct SubAgentLoopContext<'a> {
    pub session: SessionRefs<'a>,
    pub provider: &'a OpenAIProvider,
    pub parent_task_board_store_key: &'a str,
    pub message_id: &'a str,
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub enabled_skill_ids: &'a [String],
    pub task: &'a AgentTask,
    pub sub_tool_budget: &'a mut SessionToolBudget,
    pub llm_stats: &'a mut ConversationLlmStats,
    pub run_id: &'a str,
    /// Depth of this sub-agent run (lead's first child = 1).
    pub spawn_depth: u32,
    pub max_spawn_depth: u32,
}

/// Supervisor orchestration loop.
pub struct SupervisorLoopContext<'a> {
    pub session: SessionRefsArc<'a>,
    pub history: &'a mut Vec<ChatMessage>,
    pub enabled_skill_ids: &'a [String],
    pub provider: OpenAIProvider,
    pub tool_budget: &'a mut SessionToolBudget,
    pub llm_stats: &'a mut ConversationLlmStats,
    pub run_id: &'a str,
}

/// Nested `run_subagent` delegation from a tool pass.
pub struct SubagentDelegationContext<'a> {
    pub session: SessionRefs<'a>,
    pub parent_task_board_store_key: &'a str,
    pub message_id: &'a str,
    pub provider: &'a OpenAIProvider,
    pub run_id: &'a str,
    pub allow_agents: &'a [String],
    pub enabled_skill_ids: &'a [String],
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub llm_stats: &'a mut ConversationLlmStats,
    pub tool_call_id: &'a str,
    pub args_value: serde_json::Value,
    /// Depth of the agent issuing `run_subagent` (lead = 0).
    pub parent_spawn_depth: u32,
    /// Lead transcript buffer; used to persist `agent_trace` on the anchor assistant row.
    pub history: Option<&'a mut Vec<ChatMessage>>,
}
