//! Per-conversation LLM usage from chat/completions `usage` (streaming + non-stream).

use crate::agent_instance_scope::AgentInstanceScope;
use crate::models::ModelSettings;
use crate::token_usage_store;

/// Model id for token usage reporting (same string as the chat/completions `model` field).
pub fn model_name_for_usage_report(model: &str) -> Option<&str> {
    let m = model.trim();
    if m.is_empty() {
        None
    } else {
        Some(m)
    }
}

pub fn model_name_for_settings_report(settings: &ModelSettings) -> Option<&str> {
    model_name_for_usage_report(&settings.model)
}

/// One API `usage` snapshot (normalized to u32; missing fields treated as 0).
#[derive(Debug, Clone, Default)]
pub struct LlmUsageSnapshot {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    /// From `completion_tokens_details.reasoning_tokens` when present.
    pub reasoning_tokens: u32,
    /// From `prompt_tokens_details.cached_tokens` (or top-level `cached_tokens`).
    /// Part of `prompt_tokens` when the provider reports OpenAI-compatible usage.
    pub cached_tokens: u32,
}

impl LlmUsageSnapshot {
    /// Completion tokens excluding reported reasoning (best-effort).
    pub fn output_tokens(&self) -> u32 {
        self.completion_tokens.saturating_sub(self.reasoning_tokens)
    }

    /// Prompt tokens that hit context cache (clamped to `prompt_tokens`).
    pub fn cache_hit_tokens(&self) -> u32 {
        self.cached_tokens.min(self.prompt_tokens)
    }

    /// Prompt tokens not covered by reported cache hits.
    pub fn cache_miss_tokens(&self) -> u32 {
        self.prompt_tokens.saturating_sub(self.cache_hit_tokens())
    }

    /// DashScope native web search `usage` block (`input_tokens` / `output_tokens`).
    pub fn from_dashscope_web_search(
        input_tokens: u32,
        output_tokens: u32,
        total_tokens: u32,
    ) -> Self {
        Self {
            prompt_tokens: input_tokens,
            completion_tokens: output_tokens,
            total_tokens,
            reasoning_tokens: 0,
            cached_tokens: 0,
        }
    }
}

/// Accumulates one user `run_chat` session (end-of-run summary).
#[derive(Debug, Default)]
pub struct ConversationLlmStats {
    pub llm_rounds: u32,
    pub sum_prompt: u64,
    pub sum_completion: u64,
    pub sum_total: u64,
    pub sum_reasoning: u64,
    /// Sum of reported context-cache hits (`cached_tokens`) across rounds.
    pub sum_cache_hit: u64,
    /// Sum of prompt tokens not covered by reported cache hits.
    pub sum_cache_miss: u64,
    pub tool_invocations: u32,
    pub rounds_missing_usage: u32,
    /// Most recent LLM round `prompt_tokens` from API usage (this session).
    pub last_round_prompt_tokens: Option<u32>,
}

impl ConversationLlmStats {
    pub fn record_llm_round(
        &mut self,
        scope: &AgentInstanceScope,
        usage: Option<&LlmUsageSnapshot>,
        model_name: Option<&str>,
    ) {
        self.llm_rounds = self.llm_rounds.saturating_add(1);
        match usage {
            Some(u) => {
                let cache_hit = u.cache_hit_tokens();
                let cache_miss = u.cache_miss_tokens();
                self.last_round_prompt_tokens = Some(u.prompt_tokens);
                self.sum_prompt = self.sum_prompt.saturating_add(u.prompt_tokens as u64);
                self.sum_completion = self
                    .sum_completion
                    .saturating_add(u.completion_tokens as u64);
                self.sum_total = self.sum_total.saturating_add(u.total_tokens as u64);
                self.sum_reasoning = self.sum_reasoning.saturating_add(u.reasoning_tokens as u64);
                self.sum_cache_hit = self.sum_cache_hit.saturating_add(cache_hit as u64);
                self.sum_cache_miss = self.sum_cache_miss.saturating_add(cache_miss as u64);
                log::debug!(
                    "LLM round {} {} tokens: total={} prompt={} completion={} cache_hit={} cache_miss={}",
                    self.llm_rounds,
                    scope.log_suffix(),
                    u.total_tokens,
                    u.prompt_tokens,
                    u.completion_tokens,
                    cache_hit,
                    cache_miss
                );
            }
            None => {
                self.rounds_missing_usage = self.rounds_missing_usage.saturating_add(1);
            }
        }
        if let Err(e) = token_usage_store::record_round(scope, usage, model_name, None) {
            log::warn!(
                "token_usage_store: record_round failed {}: {e}",
                scope.log_suffix()
            );
        }
    }

    pub fn record_tool_invocation(&mut self) {
        self.tool_invocations = self.tool_invocations.saturating_add(1);
    }

    pub fn log_summary(&self, conversation_id: &str) {
        if self.llm_rounds == 0 && self.tool_invocations == 0 {
            return;
        }
        log::info!(
            "LLM token summary conversation_id={} llm_rounds={} total_tokens={} prompt_tokens={} cache_hit={} cache_miss={} tool_invocations={}",
            conversation_id,
            self.llm_rounds,
            self.sum_total,
            self.sum_prompt,
            self.sum_cache_hit,
            self.sum_cache_miss,
            self.tool_invocations
        );
    }
}

/// Lead-agent token session for one `run_chat`.
pub(crate) struct ChatLlmTokenSession {
    pub run_id: String,
    pub stats: ConversationLlmStats,
    pub lead_scope: AgentInstanceScope,
}

impl ChatLlmTokenSession {
    pub(crate) fn new(
        run_id: String,
        conversation_id: String,
        agent_role_id: String,
        model_name: Option<String>,
    ) -> Self {
        let lead_scope =
            AgentInstanceScope::new(run_id.clone(), conversation_id.clone(), agent_role_id);
        let _model = model_name;
        Self {
            run_id,
            stats: ConversationLlmStats::default(),
            lead_scope,
        }
    }
}

impl Drop for ChatLlmTokenSession {
    fn drop(&mut self) {
        self.stats.log_summary(&self.lead_scope.conversation_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_hit_and_miss_split_prompt_tokens() {
        let snap = LlmUsageSnapshot {
            prompt_tokens: 1000,
            cached_tokens: 750,
            ..Default::default()
        };
        assert_eq!(snap.cache_hit_tokens(), 750);
        assert_eq!(snap.cache_miss_tokens(), 250);
    }
}
