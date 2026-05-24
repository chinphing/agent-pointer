//! Per-conversation LLM usage from chat/completions `usage` (streaming + non-stream).

use crate::agent_instance_scope::AgentInstanceScope;
use crate::token_usage_store;

/// One API `usage` snapshot (normalized to u32; missing fields treated as 0).
#[derive(Debug, Clone, Default)]
pub struct LlmUsageSnapshot {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    /// From `completion_tokens_details.reasoning_tokens` when present.
    pub reasoning_tokens: u32,
}

impl LlmUsageSnapshot {
    /// Completion tokens excluding reported reasoning (best-effort).
    pub fn output_tokens(&self) -> u32 {
        self.completion_tokens.saturating_sub(self.reasoning_tokens)
    }
}

/// Accumulates one user `run_chat` session (debug summary).
#[derive(Debug, Default)]
pub struct ConversationLlmStats {
    pub llm_rounds: u32,
    pub sum_prompt: u64,
    pub sum_completion: u64,
    pub sum_total: u64,
    pub sum_reasoning: u64,
    pub tool_invocations: u32,
    pub rounds_missing_usage: u32,
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
                self.sum_prompt = self.sum_prompt.saturating_add(u.prompt_tokens as u64);
                self.sum_completion = self
                    .sum_completion
                    .saturating_add(u.completion_tokens as u64);
                self.sum_total = self.sum_total.saturating_add(u.total_tokens as u64);
                self.sum_reasoning = self
                    .sum_reasoning
                    .saturating_add(u.reasoning_tokens as u64);
                log::debug!(
                    "LLM round {} {} tokens: total={} prompt={} completion={}",
                    self.llm_rounds,
                    scope.log_suffix(),
                    u.total_tokens,
                    u.prompt_tokens,
                    u.completion_tokens
                );
            }
            None => {
                self.rounds_missing_usage = self.rounds_missing_usage.saturating_add(1);
            }
        }
        if let Err(e) = token_usage_store::record_round(scope, usage, model_name) {
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
            "LLM token summary conversation_id={} llm_rounds={} total_tokens={} tool_invocations={}",
            conversation_id,
            self.llm_rounds,
            self.sum_total,
            self.tool_invocations
        );
    }
}

/// Lead-agent token session for one `run_chat`.
pub(crate) struct ChatLlmTokenSession {
    pub stats: ConversationLlmStats,
    pub lead_scope: AgentInstanceScope,
}

impl ChatLlmTokenSession {
    pub(crate) fn new(conversation_id: String, agent_role_id: String, model_name: Option<String>) -> Self {
        let lead_scope = AgentInstanceScope::new(conversation_id.clone(), agent_role_id);
        if let Err(e) = token_usage_store::ensure_accum(&lead_scope) {
            log::warn!(
                "token_usage_store: ensure_accum failed {}: {e}",
                lead_scope.log_suffix()
            );
        }
        let _model = model_name;
        Self {
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
