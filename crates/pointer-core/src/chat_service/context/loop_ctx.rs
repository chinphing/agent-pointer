//! Agent loop entry contexts (lead single, sub, supervisor).

use crate::agents::{AgentDef, AgentPlan, AgentTask};
use crate::agent_instance_scope::AgentInstanceScope;
use crate::llm_token_stats::{ChatLlmTokenSession, ConversationLlmStats};
use crate::models::{AgentTrace, ChatMessage, ModelSettings};
use crate::provider::OpenAIProvider;
use std::sync::Arc;

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
}

/// Sub-agent run loop.
pub struct SubAgentLoopContext<'a> {
    pub session: SessionRefs<'a>,
    pub parent_task_board_store_key: &'a str,
    pub message_id: &'a str,
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub enabled_skill_ids: &'a [String],
    pub task: &'a AgentTask,
    pub sub_tool_budget: &'a mut SessionToolBudget,
    pub llm_stats: &'a mut ConversationLlmStats,
    pub run_id: &'a str,
    pub reasoning_in_messages: bool,
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
    pub reasoning_in_messages: bool,
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
}

/// Sub-agent runtime refs populated after session init.
pub struct SubRunRefs<'a> {
    pub def: &'a AgentDef,
    pub task: &'a AgentTask,
    pub instance_scope: &'a AgentInstanceScope,
    pub message_id: &'a str,
}
