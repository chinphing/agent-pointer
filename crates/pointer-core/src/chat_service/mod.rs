//! Chat session orchestration (`run_chat`), tool loops, and sub-agents.
//!
//! Small helpers live in sibling modules; the main control flow is in [`session`].

mod agent_tool_allowlist;
mod app_state;
mod content_extract;
mod emit;
mod json_tool_retries;
mod prompts;
mod provider_stream;
mod session;
mod session_budget;
mod session_model;
mod task_board_inject;
mod util;

pub type StreamTx = crate::models::ChatStreamSender;

pub use app_state::AppState;
pub use session::run_chat;
