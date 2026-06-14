//! Per-round provider stream inputs and session refs.

use crate::llm_token_stats::ChatLlmTokenSession;
use crate::models::{ChatMessage, ModelSettings, SystemPromptSections};
use tokio_util::sync::CancellationToken;

use super::session::{SessionRefs, SessionRefsArc};
use super::super::session_budget::SessionToolBudget;

/// Owned inputs assembled once per stream round (prompt clone + native tools).
pub struct StreamRoundInput {
    pub history_for_api: Vec<ChatMessage>,
    pub system_prompts: SystemPromptSections,
    pub native_tools: Vec<serde_json::Value>,
    pub tools_appendix_enabled: bool,
}

/// Lead single-agent provider stream round.
pub struct LeadStreamRoundContext<'a> {
    pub session: SessionRefsArc<'a>,
    pub settings: &'a ModelSettings,
    pub history: &'a mut Vec<ChatMessage>,
    pub token_session: &'a mut ChatLlmTokenSession,
    pub tool_budget: &'a mut SessionToolBudget,
    pub consumed_single: &'a mut u32,
    pub max_cap: u32,
    pub reasoning_in_messages: bool,
}

/// Sub-agent provider stream round (uses parent `message_id` for trace UI).
pub struct SubStreamRoundContext<'a> {
    pub session: SessionRefs<'a>,
    pub message_id: &'a str,
    pub sub_tool_budget: &'a mut SessionToolBudget,
    pub max_cap: u32,
    pub reasoning_in_messages: bool,
}

/// Cancel token clone helper for spawned provider tasks.
pub fn cancel_owned(cancel: &CancellationToken) -> CancellationToken {
    cancel.clone()
}
