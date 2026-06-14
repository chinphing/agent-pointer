//! LLM provider and per-round model settings.

use crate::models::ModelSettings;
use crate::provider::OpenAIProvider;

/// Provider + settings for one assistant stream round or post-stream decision.
pub struct LlmRoundRefs<'a> {
    pub provider: &'a OpenAIProvider,
    pub settings: &'a ModelSettings,
}
