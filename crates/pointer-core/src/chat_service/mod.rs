//! Chat session orchestration (`run_chat`), tool loops, and sub-agents.
//!
//! Small helpers live in sibling modules; entrypoint `session::run_chat`, orchestration `session_inner`,
//! single-agent `single_agent` (+ `_prompt` / `_stream` / `_post_stream` / `_tools` thin wrappers),
//! sub-agent `sub_agent` (+ `_prompt` / `_stream`),
//! shared `agent_stream_round` / `agent_post_stream` / `agent_round_lifecycle` / `agent_tool_pass` / `run_subagent_delegation`.

mod agent_post_stream;
mod agent_round_lifecycle;
mod agent_stream_round;
mod agent_tool_allowlist;
mod agent_tool_pass;
pub(crate) mod app_state;
mod computer_monitor_follow;
mod computer_monitor_pick;
mod computer_pipeline_loop;
mod content_extract;
mod context;
mod conversation_persist;
mod emit;
mod json_tool_retries;
mod prompts;
mod provider_stream;
mod run_subagent_delegation;
mod self_fork;
mod session;
mod session_budget;
mod session_inner;
pub(crate) mod session_model;
mod single_agent;
mod single_agent_post_stream;
mod single_agent_prompt;
mod single_agent_stream;
mod single_agent_tools;
mod sub_agent;
mod sub_agent_prompt;
mod sub_agent_stream;
mod sub_agent_task_prompt;
pub(crate) mod sub_message;
pub use crate::task_board::sub_agent_task_board_store_key;
mod util;

pub type StreamTx = crate::models::ChatStreamSender;

pub use app_state::AppState;
pub use app_state::GlobalMcpView;
pub use session::run_chat;
