//! Shared types for lead / sub-agent tool execution passes.

use crate::agent_instance_scope::AgentInstanceScope;
use crate::agents::{AgentDef, AgentProfile, AgentRunResult, AgentTask};
use crate::dispatcher::TriggerSource;
use crate::llm_token_stats::{ChatLlmTokenSession, ConversationLlmStats};
use crate::models::{AgentTrace, ChatMessage, ModelSettings, ToolCall};
use crate::provider::OpenAIProvider;
use crate::task_board::TaskBoardTrimHook;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::super::app_state::AppState;
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
    /// Deprecated: previously used for non-blocking IM `ask_user`. Kept for match exhaustiveness.
    AskUserDeferred,
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

    pub fn conversation_stats_mut(&mut self) -> &mut ConversationLlmStats {
        match self {
            ToolInvocationStats::TokenSession(session) => &mut session.stats,
            ToolInvocationStats::Conversation(stats) => stats,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ActiveAgentExecutionState<'a> {
    pub def: &'a AgentDef,
    pub system_prompt: &'a str,
    pub skill_ids: &'a [String],
    pub skill_prompts: &'a [String],
    pub allowed_tools: &'a [String],
}

pub struct LeadToolPassConfig<'a> {
    pub run_id: &'a str,
    pub instance_scope: &'a AgentInstanceScope,
    pub allow_agents: &'a [String],
    pub enabled_skill_ids: &'a mut Vec<String>,
    pub agent_skill_overrides: &'a std::collections::HashMap<String, Vec<String>>,
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub file_tool_lead_for_invoke: AgentProfile,
    pub lead_agent_id: &'a str,
    pub active: ActiveAgentExecutionState<'a>,
}

pub struct SubToolPassConfig<'a> {
    pub task: &'a AgentTask,
    pub allow_agents: &'a [String],
    pub agent_skill_overrides: &'a std::collections::HashMap<String, Vec<String>>,
    pub instance_scope: &'a AgentInstanceScope,
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub accumulated_content: String,
    pub trace_id: String,
    pub spawn_depth: u32,
    pub scoped_message_id: String,
    pub active: ActiveAgentExecutionState<'a>,
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
    /// Origin of the parent run; used to branch tool behavior (e.g. IM `ask_user` timeout).
    pub trigger_source: Option<TriggerSource>,
    /// Legacy flag; IM `ask_user` now blocks same-turn (unused).
    pub ask_user_deferred: AtomicBool,
    /// Process-lifetime `AppState` for background jobs that outlive this pass.
    pub state_arc: Arc<AppState>,
}

impl<'a> ToolPassContext<'a> {
    pub fn persist_transcript(&self) -> bool {
        self.persist.persist_transcript()
    }

    /// Id of the agent running this pass and the worker ids it may delegate to.
    /// A delegated sub-agent scope wins: it is the one calling `run_subagent`.
    pub fn active_delegation_scope(&self) -> Option<(&str, &[String])> {
        if let Some(sub) = self.sub.as_ref() {
            return Some((sub.active.def.id.as_str(), sub.allow_agents));
        }
        self.lead
            .as_ref()
            .map(|lead| (lead.active.def.id.as_str(), lead.allow_agents))
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
    pub instance_scope: &'a AgentInstanceScope,
    pub active: ActiveAgentExecutionState<'a>,
    pub file_tool_lead_for_invoke: AgentProfile,
    pub assistant_id: String,
    pub final_tool_calls: &'a [ToolCall],
    pub agent_trace: &'a mut Vec<AgentTrace>,
    pub cancel: CancellationToken,
    pub state_arc: Arc<AppState>,
    /// Origin of the parent run; used to branch tool behavior per source
    /// (e.g. non-blocking `ask_user` for IM channels).
    pub trigger_source: Option<TriggerSource>,
}

/// One tool-pass invocation: validated tool batch + optional trim hook.
pub struct ToolPassRequest<'a> {
    pub ctx: ToolPassContext<'a>,
    pub final_tool_calls: &'a [ToolCall],
    pub trim_hook: Option<TaskBoardTrimHook<'a>>,
    pub cancel: CancellationToken,
}
