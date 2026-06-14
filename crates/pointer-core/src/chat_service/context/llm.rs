//! LLM provider and per-round model settings.

use crate::llm_token_stats::{ChatLlmTokenSession, ConversationLlmStats};
use crate::models::ModelSettings;
use crate::provider::OpenAIProvider;

/// Provider + settings for one assistant stream round or post-stream decision.
pub struct LlmRoundRefs<'a> {
    pub provider: &'a OpenAIProvider,
    pub settings: &'a ModelSettings,
}

/// Lead single-agent token session (includes run_id and lead scope).
pub struct LeadLlmSession<'a> {
    pub llm: LlmRoundRefs<'a>,
    pub token_session: &'a mut ChatLlmTokenSession,
    pub reasoning_in_messages: bool,
}

/// Sub-agent or supervisor conversation-level LLM stats.
pub struct ConversationLlmRefs<'a> {
    pub llm: LlmRoundRefs<'a>,
    pub stats: &'a mut ConversationLlmStats,
    pub reasoning_in_messages: bool,
}
