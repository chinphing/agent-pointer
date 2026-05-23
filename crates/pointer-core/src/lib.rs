pub mod agents;
pub mod chat_service;
pub mod env_prompt;
pub mod platform;
pub mod platform_auth;
pub mod platform_endpoints;
pub mod token_usage_store;
pub mod extensions;
pub mod context_compression;
pub mod task_board;

/// Deprecated re-export — use [`task_board::history_trim`] instead.
pub mod task_board_history_trim {
    pub use crate::task_board::history_trim::*;
}
pub mod local_secret;
pub mod logging;
pub mod llm_prompt_dump;
pub mod llm_token_stats;
pub mod models;
pub mod platform_config;
pub mod provider;
pub mod skills;
pub mod storage;
pub mod tools;
pub mod tool_envelope;
pub mod json_tool_caller;
pub(crate) mod json_interior_quote_escape;
pub mod tools_system_appendix;
