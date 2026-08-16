//! Token accounting for DashScope web-search LLM rounds.

use crate::agent_instance_scope::AgentInstanceScope;
use crate::llm_token_stats::LlmUsageSnapshot;
use crate::token_usage_store;

use super::client::WebSearchResult;

/// Parallel-safe web-search usage recorder.
///
/// Web-search model calls are auxiliary calls, so they are persisted for usage
/// reporting without changing the agent-loop prompt size used by compression.
pub struct WebSearchTokenRecorder {
    scope: AgentInstanceScope,
    source: &'static str,
}

impl WebSearchTokenRecorder {
    pub fn new(scope: AgentInstanceScope, source: &'static str) -> Self {
        Self { scope, source }
    }

    pub fn record(&self, result: &WebSearchResult) {
        if result.usage.total_tokens == 0 {
            log::warn!(
                "web_search token usage missing usage payload {} model={}",
                self.scope.log_suffix(),
                result.model
            );
            return;
        }
        let snapshot = LlmUsageSnapshot::from_dashscope_web_search(
            result.usage.input_tokens,
            result.usage.output_tokens,
            result.usage.total_tokens,
        );
        let model = result.model.as_str();
        if let Err(e) = token_usage_store::record_round(
            &self.scope,
            Some(&snapshot),
            Some(model),
            None,
            self.source,
        ) {
            log::warn!(
                "token_usage_store: web_search record_round failed {} model={model}: {e}",
                self.scope.log_suffix()
            );
        } else {
            log::info!(
                "web_search token usage {} model={} total={} prompt={} completion={}",
                self.scope.log_suffix(),
                model,
                snapshot.total_tokens,
                snapshot.prompt_tokens,
                snapshot.completion_tokens
            );
        }
    }
}
