//! Shared types for lead / sub-agent tool execution passes.

use crate::agent_instance_scope::AgentInstanceScope;
use crate::agents::{AgentDef, AgentProfile, AgentRunResult, AgentTask};
use crate::llm_token_stats::{ChatLlmTokenSession, ConversationLlmStats};
use crate::models::AgentTrace;

/// Outcome of executing a non-empty validated tool batch for one assistant turn.
#[derive(Debug)]
pub enum ToolPassResult {
    /// Sub-agent: tool pass ended without executing tools (legacy exit; prefer `FinishRun` in sub loop).
    SubFinished(AgentRunResult),
    /// Lone successful [`ToolEntry::final_reply`] tool — host delivers output as the final message.
    FinalReplyComplete(String),
    /// No tool ran to completion in a way that consumes a round (synced out for lead).
    NoopExit,
    /// At least one tool produced results; caller should record a tool cycle and check budget.
    RanTools,
}

pub enum ToolInvocationStats<'a> {
    TokenSession(&'a mut ChatLlmTokenSession),
    Conversation(&'a mut ConversationLlmStats),
}

impl ToolInvocationStats<'_> {
    pub fn record_tool_invocation(&mut self) {
        match self {
            ToolInvocationStats::TokenSession(s) => s.stats.record_tool_invocation(),
            ToolInvocationStats::Conversation(s) => s.record_tool_invocation(),
        }
    }
}

pub struct LeadToolPassConfig<'a> {
    pub run_id: &'a str,
    pub allow_agents: &'a [String],
    pub enabled_skill_ids: &'a mut Vec<String>,
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub file_tool_lead_for_invoke: AgentProfile,
    pub lead_agent_id: &'a str,
}

pub struct SubToolPassConfig<'a> {
    pub def: &'a AgentDef,
    pub task: &'a AgentTask,
    pub allowed_tools: &'a [String],
    pub allow_agents: &'a [String],
    pub instance_scope: &'a AgentInstanceScope,
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub accumulated_content: String,
    pub accumulated_reasoning: String,
    pub reasoning_in_messages: bool,
    pub trace_id: String,
}

pub(super) type ToolExecResult = Result<(String, bool, Option<String>), anyhow::Error>;
