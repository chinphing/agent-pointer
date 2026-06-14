//! Prompt assembly context for lead and sub-agent rounds.

use crate::agents::{AgentDef, AgentPlan, AgentProfile};
use std::sync::Arc;

use super::session::{SessionRefs, SessionRefsArc};
use super::super::app_state::AppState;
use crate::models::{ChatMessage, ModelSettings};

/// Lead single-agent round prompt assembly.
pub struct SingleAgentPromptContext<'a> {
    pub session: SessionRefsArc<'a>,
    pub history: &'a [ChatMessage],
    pub agent_plan: &'a AgentPlan,
    pub settings: &'a ModelSettings,
    pub main_task_board_store_key: &'a str,
    pub assistant_id: &'a str,
    pub lead_profile: AgentProfile,
    pub tools_system_appendix: String,
    pub tools_appendix_enabled: bool,
}

/// Sub-agent round prompt assembly.
pub struct SubAgentPromptContext<'a> {
    pub session: SessionRefs<'a>,
    pub message_id: &'a str,
    pub task_id: &'a str,
    pub round_message_id: &'a str,
    pub local_history: &'a [ChatMessage],
    pub session_extras: &'a [String],
    pub tools_system_appendix: &'a str,
    pub sub_task_board_key: &'a str,
    pub def: &'a AgentDef,
    pub workspace_root: &'a str,
    pub user_dynamic_inject_enabled: bool,
}

/// Convenience builder when only `Arc<AppState>` is available at the call site.
pub struct PromptSessionArc<'a> {
    pub state: Arc<AppState>,
    pub stream: &'a super::super::StreamTx,
    pub conversation_id: &'a str,
}
