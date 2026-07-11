//! Chat session orchestration (`run_chat`), tool loops, and sub-agents.
//!
//! Small helpers live in sibling modules; entrypoint `session::run_chat`, orchestration `session_inner`,
//! single-agent `single_agent` (+ `_prompt` / `_stream` / `_post_stream` / `_tools` thin wrappers),
//! sub-agent `sub_agent` (+ `_prompt` / `_stream`), supervisor `supervisor` (+ `_plan` / `_synth`),
//! shared `agent_stream_round` / `agent_post_stream` / `agent_round_lifecycle` / `agent_tool_pass` / `run_subagent_delegation`.

mod context;
mod computer_monitor_pick;
mod computer_monitor_follow;
mod computer_pipeline_loop;
mod conversation_persist;
mod agent_post_stream;
mod agent_round_lifecycle;
mod agent_stream_round;
mod agent_tool_pass;
mod agent_tool_allowlist;
mod app_state;
mod content_extract;
mod emit;
mod json_tool_retries;
mod prompts;
mod provider_stream;
mod run_subagent_delegation;
mod session_inner;
mod session;
mod single_agent;
mod single_agent_prompt;
mod single_agent_stream;
mod single_agent_post_stream;
mod single_agent_tools;
mod sub_agent;
mod sub_agent_prompt;
mod sub_agent_stream;
mod sub_agent_task_prompt;
mod sub_message;
mod supervisor;
mod supervisor_plan;
mod supervisor_synth;
mod session_budget;
mod session_model;
pub use crate::task_board::sub_agent_task_board_store_key;
mod util;

pub type StreamTx = crate::models::ChatStreamSender;

pub use app_state::AppState;
pub use session::run_chat;
pub(crate) use emit::{emit, emit_task_board_updated, trace_id_opt};
pub(crate) use util::{
    patch_assistant_tool_call_display, patch_assistant_tool_call_outcome,
    tool_display_stream_fields,
};
pub(crate) use session_model::resolve_provider_api_key;
