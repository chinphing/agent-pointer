//! Top-level chat session entry (`run_chat` → `run_chat_inner`).

use crate::models::ChatMessage;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::session::SessionRefsArc;
use super::super::app_state::AppState;
use super::super::StreamTx;

/// Immutable request payload from Tauri / HTTP boundary into `run_chat_inner`.
pub struct ChatRunRequest {
    pub agent_mode: Option<String>,
    pub lead_agent_id_override: Option<String>,
    pub tool_rounds_used_single_start: u32,
    pub tool_rounds_used_supervisor_start: u32,
    pub workspace_root: String,
    pub run_id: String,
}

/// Mutable session state for one `run_chat_inner` invocation.
pub struct ChatRunContext<'a> {
    pub stream: StreamTx,
    pub state: Arc<AppState>,
    pub conversation_id: &'a str,
    pub history: &'a mut Vec<ChatMessage>,
    pub enabled_skill_ids: &'a mut Vec<String>,
    pub consumed_single: &'a mut u32,
    pub consumed_supervisor: &'a mut u32,
    pub cancel: CancellationToken,
}

impl<'a> ChatRunContext<'a> {
    pub fn session_arc(&'a self) -> SessionRefsArc<'a> {
        SessionRefsArc {
            stream: &self.stream,
            state: self.state.clone(),
            conversation_id: self.conversation_id,
            cancel: self.cancel.clone(),
        }
    }
}
