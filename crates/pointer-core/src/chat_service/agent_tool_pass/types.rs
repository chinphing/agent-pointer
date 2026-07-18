//! Shared types for lead / sub-agent tool execution passes.

use crate::agent_instance_scope::AgentInstanceScope;
use crate::agents::{AgentDef, AgentProfile, AgentRunResult, AgentTask};
use crate::llm_token_stats::{ChatLlmTokenSession, ConversationLlmStats};
use crate::models::{AgentTrace, ChatMessage, ModelSettings, ToolCall};
use crate::provider::OpenAIProvider;
use crate::task_board::TaskBoardTrimHook;
use tokio_util::sync::CancellationToken;

use super::super::context::{SessionRefs, TranscriptPersist, TranscriptRefs};
use super::super::session_budget::SessionToolBudget;

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
    pub agent_skill_overrides: &'a std::collections::HashMap<String, Vec<String>>,
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub file_tool_lead_for_invoke: AgentProfile,
    pub lead_agent_id: &'a str,
}

pub struct SubToolPassConfig<'a> {
    pub def: &'a AgentDef,
    pub task: &'a AgentTask,
    pub allowed_tools: &'a [String],
    pub allow_agents: &'a [String],
    pub agent_skill_overrides: &'a std::collections::HashMap<String, Vec<String>>,
    pub instance_scope: &'a AgentInstanceScope,
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub accumulated_content: String,
    pub accumulated_reasoning: String,
    pub reasoning_in_messages: bool,
    pub trace_id: String,
    pub spawn_depth: u32,
    pub scoped_message_id: String,
}

pub(super) type ToolExecResult = Result<(String, bool, Option<String>), anyhow::Error>;

/// Shared inputs for `run_agent_tool_pass` and dispatch helpers.
pub struct ToolPassContext<'a> {
    pub session: SessionRefs<'a>,
    pub transcript: TranscriptRefs<'a>,
    pub persist: TranscriptPersist,
    pub message_id: String,
    pub task_board_store_key: &'a str,
    pub tool_approval_mode: &'a str,
    pub tool_budget: &'a mut SessionToolBudget,
    pub consumed_single: Option<&'a mut u32>,
    pub provider: &'a OpenAIProvider,
    pub stats: &'a mut ToolInvocationStats<'a>,
    pub lead: Option<LeadToolPassConfig<'a>>,
    pub sub: Option<SubToolPassConfig<'a>>,
    pub task_board_work_items_enabled: bool,
    pub workspace_root: &'a str,
}

impl<'a> ToolPassContext<'a> {
    pub fn persist_transcript(&self) -> bool {
        self.persist.persist_transcript()
    }
}

/// Lead single-agent tool pass request (wrapper over shared `run_agent_tool_pass`).
pub struct LeadSingleToolPassRequest<'a> {
    pub session: SessionRefs<'a>,
    pub main_task_board_store_key: &'a str,
    pub history: &'a mut Vec<ChatMessage>,
    pub allow_agents: &'a [String],
    pub enabled_skill_ids: &'a mut Vec<String>,
    pub agent_skill_overrides: &'a std::collections::HashMap<String, Vec<String>>,
    pub provider: &'a OpenAIProvider,
    pub tool_approval_mode: &'a str,
    pub tool_budget: &'a mut SessionToolBudget,
    pub consumed_single: &'a mut u32,
    pub token_session: &'a mut ChatLlmTokenSession,
    pub settings: &'a ModelSettings,
    pub lead_agent_id: &'a str,
    pub file_tool_lead_for_invoke: AgentProfile,
    pub assistant_id: String,
    pub final_tool_calls: &'a [ToolCall],
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub cancel: CancellationToken,
}

/// One tool-pass invocation: validated tool batch + optional trim hook.
pub struct ToolPassRequest<'a> {
    pub ctx: ToolPassContext<'a>,
    pub final_tool_calls: &'a [ToolCall],
    pub trim_hook: Option<TaskBoardTrimHook<'a>>,
    pub cancel: CancellationToken,
}
