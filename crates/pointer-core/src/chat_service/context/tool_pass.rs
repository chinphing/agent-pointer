//! Tool execution pass request context.

use crate::agents::AgentProfile;
use crate::llm_token_stats::ChatLlmTokenSession;
use crate::models::{AgentTrace, ChatMessage, ModelSettings, ToolCall};
use crate::provider::OpenAIProvider;
use crate::task_board::TaskBoardTrimHook;
use tokio_util::sync::CancellationToken;

use super::super::agent_tool_pass::{
    LeadToolPassConfig, SubToolPassConfig, ToolInvocationStats,
};
use super::session::SessionRefs;
use super::super::session_budget::SessionToolBudget;
use super::transcript::{TranscriptPersist, TranscriptRefs};

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

/// One tool-pass invocation: validated tool batch + lead/sub config + optional trim hook.
pub struct ToolPassRequest<'a> {
    pub ctx: ToolPassContext<'a>,
    pub final_tool_calls: &'a [ToolCall],
    pub lead: Option<LeadToolPassConfig<'a>>,
    pub sub: Option<SubToolPassConfig<'a>>,
    pub trim_hook: Option<TaskBoardTrimHook<'a>>,
    pub cancel: CancellationToken,
}

/// Per-tool dispatch within a pass (approval + execute + outcome).
pub struct ToolInvocationContext<'a> {
    pub pass: &'a mut ToolPassContext<'a>,
    pub tc: &'a ToolCall,
    pub tool_id: String,
    pub args_value: serde_json::Value,
    pub lead: Option<&'a mut LeadToolPassConfig<'a>>,
    pub sub: Option<&'a mut SubToolPassConfig<'a>>,
    pub cancel: &'a CancellationToken,
}
