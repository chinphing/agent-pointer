//! Per-conversation LLM usage from chat/completions `usage` (streaming + non-stream).

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

/// Accumulates one user `run_chat` session (single-agent loop and/or supervisor subtree).
#[derive(Debug, Default)]
pub struct ConversationLlmStats {
    pub llm_rounds: u32,
    pub sum_prompt: u64,
    pub sum_completion: u64,
    pub sum_total: u64,
    pub sum_reasoning: u64,
    /// Non-`response` tool executions (after validation, including failed invoke).
    pub tool_invocations: u32,
    pub rounds_missing_usage: u32,
}

impl ConversationLlmStats {
    pub fn record_llm_round(&mut self, usage: Option<&LlmUsageSnapshot>) {
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
                let out = u.output_tokens();
                log::debug!(
                    "LLM round {} tokens: total={} prompt={} completion={} reasoning={} output={}",
                    self.llm_rounds,
                    u.total_tokens,
                    u.prompt_tokens,
                    u.completion_tokens,
                    u.reasoning_tokens,
                    out
                );
            }
            None => {
                self.rounds_missing_usage = self.rounds_missing_usage.saturating_add(1);
                log::debug!(
                    "LLM round {} finished without usage (enable stream_options.include_usage on the provider; set POINTER_STREAM_INCLUDE_USAGE=0 to omit the request field)",
                    self.llm_rounds
                );
            }
        }
    }

    pub fn record_tool_invocation(&mut self) {
        self.tool_invocations = self.tool_invocations.saturating_add(1);
    }

    pub fn log_summary(&self, conversation_id: &str) {
        if self.llm_rounds == 0 && self.tool_invocations == 0 {
            return;
        }
        let output_sum = self.sum_completion.saturating_sub(self.sum_reasoning);
        let avg_per_tool = if self.tool_invocations > 0 {
            Some(self.sum_total as f64 / self.tool_invocations as f64)
        } else {
            None
        };
        match avg_per_tool {
            Some(avg) => {
                log::info!(
                    "LLM token summary conversation_id={} tool_invocations={} llm_rounds={} total_tokens={} prompt_tokens={} reasoning_tokens={} output_tokens={} avg_tokens_per_tool={:.2} rounds_missing_usage={}",
                    conversation_id,
                    self.tool_invocations,
                    self.llm_rounds,
                    self.sum_total,
                    self.sum_prompt,
                    self.sum_reasoning,
                    output_sum,
                    avg,
                    self.rounds_missing_usage
                );
            }
            None => {
                log::info!(
                    "LLM token summary conversation_id={} tool_invocations=0 llm_rounds={} total_tokens={} prompt_tokens={} reasoning_tokens={} output_tokens={} rounds_missing_usage={}",
                    conversation_id,
                    self.llm_rounds,
                    self.sum_total,
                    self.sum_prompt,
                    self.sum_reasoning,
                    output_sum,
                    self.rounds_missing_usage
                );
            }
        }
    }
}

/// On drop, logs [`ConversationLlmStats::log_summary`] for this `run_chat` session.
pub(crate) struct ChatLlmTokenSession {
    pub stats: ConversationLlmStats,
    conversation_id: String,
}

impl ChatLlmTokenSession {
    pub(crate) fn new(conversation_id: String) -> Self {
        Self {
            stats: ConversationLlmStats::default(),
            conversation_id,
        }
    }
}

impl Drop for ChatLlmTokenSession {
    fn drop(&mut self) {
        self.stats.log_summary(&self.conversation_id);
    }
}
