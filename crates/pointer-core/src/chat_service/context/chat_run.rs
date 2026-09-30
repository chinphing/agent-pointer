//! Top-level chat session entry (`run_chat` → `run_chat_inner`).

use crate::dispatcher::TriggerSource;
use crate::models::ChatMessage;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::super::app_state::AppState;
use super::super::StreamTx;

/// Immutable request payload from Tauri / HTTP boundary into `run_chat_inner`.
pub struct ChatRunRequest {
    pub lead_agent_id_override: Option<String>,
    /// Per-conversation performance tier override; `None` = global default.
    pub performance_mode_override: Option<String>,
    pub agent_skill_overrides: HashMap<String, Vec<String>>,
    pub tool_rounds_used_single_start: u32,
    pub workspace_root: String,
    /// Frontend override; when `Some(true)` skip inheriting another conversation's workspace.
    pub workspace_inherit_disabled: Option<bool>,
    pub run_id: String,
    /// Set for dispatcher-driven runs (webhook/cron); `None` for interactive chat.
    pub trigger_source: Option<TriggerSource>,
    /// Cron (or future trigger) requested IM auto-delivery of the final reply.
    /// Non-cron triggers with this set get `auto_deliver_system_prompt`.
    /// Cron ticks prepend Hermes-style guidance onto the user message instead.
    pub im_auto_deliver: bool,
}

/// Mutable session state for one `run_chat_inner` invocation.
pub struct ChatRunContext<'a> {
    pub stream: StreamTx,
    pub state: Arc<AppState>,
    pub conversation_id: &'a str,
    pub history: &'a mut Vec<ChatMessage>,
    pub enabled_skill_ids: &'a mut Vec<String>,
    pub consumed_single: &'a mut u32,
    pub cancel: CancellationToken,
}
