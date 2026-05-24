//! Token accounting for DashScope web-search LLM rounds.

use crate::agent_instance_scope::AgentInstanceScope;
use crate::llm_token_stats::{ChatLlmTokenSession, ConversationLlmStats, LlmUsageSnapshot};

use super::client::WebSearchResult;

/// Where to record web-search token usage (lead session vs sub-agent stats).
pub enum WebSearchTokenRecorder<'a> {
    Lead(&'a mut ChatLlmTokenSession),
    Sub {
        stats: &'a mut ConversationLlmStats,
        scope: &'a AgentInstanceScope,
    },
}

impl WebSearchTokenRecorder<'_> {
    pub fn record(&mut self, result: &WebSearchResult) {
        if result.usage.total_tokens == 0 {
            return;
        }
        let snapshot = LlmUsageSnapshot::from_dashscope_web_search(
            result.usage.input_tokens,
            result.usage.output_tokens,
            result.usage.total_tokens,
        );
        let model = result.model.as_str();
        match self {
            WebSearchTokenRecorder::Lead(s) => {
                s.stats
                    .record_llm_round(&s.lead_scope, Some(&snapshot), Some(model));
            }
            WebSearchTokenRecorder::Sub { stats, scope } => {
                stats.record_llm_round(scope, Some(&snapshot), Some(model));
            }
        }
    }
}
