pub mod agent_instance_scope;
pub mod agents;
pub mod channel_outbound;
pub mod chat_service;
pub mod client_env;
pub mod dotenv;
pub mod conversation_snapshot;
pub mod env_prompt;
pub mod platform;
pub mod platform_auth;
pub mod platform_endpoints;
pub mod experiences;
pub mod token_usage_store;
pub mod workspace_delegation;
pub mod extensions;
pub mod context_compression;
pub mod conversation_store;
pub mod conversation_transcript;
pub mod memory;
pub mod session_search;
pub mod message_context;
pub mod task_board;

/// Deprecated re-export — use [`task_board::history_trim`] instead.
pub mod task_board_history_trim {
    pub use crate::task_board::history_trim::*;
}
pub mod local_secret;
pub mod logging;
pub mod llm_prompt_dump;
pub mod llm_token_stats;
pub mod media;
pub mod media_generation;
pub mod mode_llm;
pub mod models;
pub mod platform_config;
pub mod provider;
pub mod shell_env;
pub mod skills;
pub mod storage;
pub mod stream_broadcast;
pub mod tools;
pub mod tool_envelope;
pub mod json_tool_caller;
pub(crate) mod json_interior_quote_escape;
pub mod tools_system_appendix;
