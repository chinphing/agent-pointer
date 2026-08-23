//! When conversation history grows past an estimated token budget, replace an older prefix
//! with a single summary message (see settings).

mod budget;
mod precompress;
mod run;
mod summary;
mod types;

#[cfg(test)]
mod tests;

pub use budget::{
    compression_gate_tokens, compute_summary_max_tokens, current_turn_token_share,
    estimate_message_payload_tokens, estimate_one_message_payload_tokens,
    estimate_text_tokens_heuristic, evaluate_compress_gate, find_in_run_drop_range,
    find_suffix_start_for_token_share, find_summary_split, find_tail_start,
    is_compression_summary_content, is_context_overflow_error, normalize_context_budget_tokens,
    plan_compression, precompress_gate_threshold, should_precompress_history,
    should_use_in_run_compression, sub_agent_between_round_compress_soft, suffix_token_share,
    summary_max_tokens_requested, summary_max_tokens_retry, summary_max_tokens_retry_prompt,
    tail_token_ratio, CompressGateDecision, CompressionPlan, COMPRESSIBLE_MIN_RATIO,
    COMPRESSION_SUMMARY_REASONING, DROP_FALLBACK_KEEP_USER_TURNS, IN_RUN_TURN_TOKEN_RATIO,
    MAX_OVERFLOW_RECOVERIES, OVERFLOW_TAIL_TOKEN_RATIO, PRECOMPRESS_GATE_RATIO,
    SUMMARY_PREFIX_BUDGET, SUMMARY_PREFIX_TOOL_LIMIT, TAIL_TOKEN_RATIO,
};
pub use precompress::{
    discard_pending_compression, maybe_spawn_precompress, prepare_history_between_llm_rounds,
    remember_session_llm, try_apply_pending_compression, try_apply_pending_compression_live,
};
pub(crate) use precompress::{
    discard_pending_compression_for_sub_agent, prepare_sub_agent_history_between_llm_rounds,
    session_llm_for_conversation,
};
pub use run::{
    maybe_compress_after_tool_round_limit, maybe_compress_history, recover_history_after_overflow,
};
pub(crate) use types::{sub_agent_compression_queue_key, SubAgentPrecompressLease};
pub use types::{CompressionScope, CompressionUiContext};
