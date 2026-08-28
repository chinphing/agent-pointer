//! Token budget, gates, and prefix vs in-run split.

use crate::models::{ChatMessage, ModelSettings, Role};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

pub(crate) const MAX_PREFIX_CHARS_FOR_API: usize = 100_000;
pub(crate) const MAX_USER_SNIPPET_CHARS: usize = 4_000;
pub(crate) const MAX_ASSISTANT_SNIPPET_CHARS: usize = 2_500;
pub(crate) const MAX_TOOL_SNIPPET_CHARS: usize = 2_000;
pub(crate) const MAX_TOOL_ARGS_CHARS: usize = 1_200;
pub(crate) const MAX_TOOL_ERROR_CHARS: usize = 1_000;
pub(crate) const MAX_REASONING_SNIPPET_CHARS: usize = 1_000;

/// Prefix on summary user rows after compression (UI detects this for dedicated styling).
pub const SUMMARY_PREFIX_BUDGET: &str = "[Conversation summary (auto-compression)]";
pub const SUMMARY_PREFIX_TOOL_LIMIT: &str =
    "[Conversation summary (auto-compression after tool rounds)]";

/// Host-written stand-in for in-run summary `reasoning_content`.
/// Thinking-mode providers (DeepSeek) reject assistant rows that omit it.
pub const COMPRESSION_SUMMARY_REASONING: &str = "[context compression]";

pub fn is_compression_summary_content(content: &str) -> bool {
    let t = content.trim_start();
    t.starts_with(SUMMARY_PREFIX_BUDGET) || t.starts_with(SUMMARY_PREFIX_TOOL_LIMIT)
}

/// Summary output budget: `content_tokens × ratio`, clamped.
/// Prompt target; API cap is this value × [`SUMMARY_MAX_TOKENS_OVERRIDE_RATIO`].
pub(crate) const SUMMARY_TOKEN_RATIO: f64 = 0.20;
pub(crate) const MIN_SUMMARY_TOKENS: u32 = 1_500;
/// Absolute ceiling for the summary *budget* (prompt target), not the API cap.
pub(crate) const SUMMARY_TOKENS_CEILING: u32 = 12_000;
/// Provider `max_tokens` headroom over the summary budget (first attempt).
/// The budget is the *target* length written into the prompt; the API cap
/// gets extra room so the model can close out without `finish_reason=length`.
pub(crate) const SUMMARY_MAX_TOKENS_OVERRIDE_RATIO: f64 = 1.5;
/// Retry prompt target as a multiple of the summary budget (`finish_reason=length`).
/// API cap is [`SUMMARY_RETRY_TOKENS_RATIO`]; keep prompt < cap so the model
/// can close out instead of filling `max_tokens`.
pub(crate) const SUMMARY_RETRY_PROMPT_TOKENS_RATIO: f64 = 2.0;
/// Retry `max_tokens` as a multiple of the summary budget when truncated
/// (`finish_reason=length`).
pub(crate) const SUMMARY_RETRY_TOKENS_RATIO: f64 = 3.0;
/// On summary LLM failure, keep this many recent real user turns (user row +
/// concluding assistant) inside the drop window instead of discarding them.
pub const DROP_FALLBACK_KEEP_USER_TURNS: usize = 3;

/// Background precompress starts once gate tokens exceed this fraction of the
/// hard context budget (still compresses when already over budget).
pub const PRECOMPRESS_GATE_RATIO: f64 = 0.80;
/// Compress only when the droppable prefix is at least this share of
/// **payload** tokens. Used only on the local-estimate fallback path
/// (no provider `prompt_tokens`) **and** when the current user turn is
/// not large enough in tokens to take the in-run path.
pub const COMPRESSIBLE_MIN_RATIO: f64 = 0.30;
/// If the latest real user message through the newest message is this
/// share of working-set **payload tokens**, summarize inside that turn
/// instead of the older prefix.
pub const IN_RUN_TURN_TOKEN_RATIO: f64 = 0.70;
/// Verbatim tail as a fraction of working-set **payload tokens**.
pub const TAIL_TOKEN_RATIO: f64 = 0.20;
/// Tighter tail when recovering from a provider overflow.
pub const OVERFLOW_TAIL_TOKEN_RATIO: f64 = 0.12;
/// Max consecutive overflow recoveries in one lead/sub-agent loop.
/// Each recovery compresses history and **retries the LLM**.
/// A successful LLM round resets the streak so a long tool loop is not
/// killed by earlier recoveries.
pub const MAX_OVERFLOW_RECOVERIES: u32 = 3;
/// Hold this many tokens back from `window - max_tokens` so a prompt of
/// exactly `window - max_tokens` does not 400 (vLLM rejects at equality).
pub const PROMPT_BUDGET_MARGIN_TOKENS: usize = 64;

/// `recoveries` is how many compress+retry cycles already ran in this streak.
pub fn should_recover_after_overflow(recoveries: u32) -> bool {
    recoveries < MAX_OVERFLOW_RECOVERIES
}

/// Dynamic summary `max_tokens`: `content × ratio`, floored at
/// [`MIN_SUMMARY_TOKENS`], capped at [`SUMMARY_TOKENS_CEILING`].
pub fn compute_summary_max_tokens(content_tokens: usize) -> u32 {
    let by_content = ((content_tokens as f64) * SUMMARY_TOKEN_RATIO).ceil() as u32;
    by_content.clamp(MIN_SUMMARY_TOKENS, SUMMARY_TOKENS_CEILING)
}

/// Provider-side `max_tokens` for the summary call: budget × 1.5.
/// The budget stays the *target* length written into the prompt; the API cap
/// gets extra room to close out instead of being truncated with
/// `finish_reason=length`.
pub fn summary_max_tokens_requested(budget: u32) -> u32 {
    ((budget as f64) * SUMMARY_MAX_TOKENS_OVERRIDE_RATIO).ceil() as u32
}

/// Retry prompt target after a `finish_reason=length` rejection: budget × 2.
pub fn summary_max_tokens_retry_prompt(budget: u32) -> u32 {
    ((budget as f64) * SUMMARY_RETRY_PROMPT_TOKENS_RATIO).ceil() as u32
}

/// Retry `max_tokens` after a `finish_reason=length` rejection: budget × 3.
pub fn summary_max_tokens_retry(budget: u32) -> u32 {
    ((budget as f64) * SUMMARY_RETRY_TOKENS_RATIO).ceil() as u32
}

/// Soft gate for background precompress: above this fraction of budget.
pub fn precompress_gate_threshold(budget_tokens: usize) -> usize {
    ((budget_tokens as f64) * PRECOMPRESS_GATE_RATIO)
        .ceil()
        .max(1.0) as usize
}

fn remembered_windows() -> &'static Mutex<HashMap<String, usize>> {
    static WINDOWS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
    WINDOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn logged_prompt_budget_clamps() -> &'static Mutex<HashSet<String>> {
    static LOGGED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    LOGGED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn model_window_cache_key(settings: &ModelSettings) -> String {
    match crate::models::active_provider_and_model(settings) {
        Some((p, m)) => format!("{}:{m}", p.id),
        None => format!("{}:{}", settings.active_provider_id, settings.model),
    }
}

fn lock_mutex<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Learned model window from a prior overflow, if any.
pub fn remembered_model_context_window(settings: &ModelSettings) -> Option<usize> {
    let key = model_window_cache_key(settings);
    lock_mutex(remembered_windows()).get(&key).copied()
}

/// Remember `maximum context length is N` from a provider overflow.
pub fn remember_model_context_window(settings: &ModelSettings, window: usize) {
    if window < 4096 {
        log::warn!(
            "context_compress: ignore tiny remembered window={} provider={} model={}",
            window,
            settings.active_provider_id,
            settings.model
        );
        return;
    }
    let key = model_window_cache_key(settings);
    let mut map = lock_mutex(remembered_windows());
    if map.get(&key) == Some(&window) {
        return;
    }
    log::info!(
        "context_compress: remember model window provider={} model={} window_tokens={}",
        settings.active_provider_id,
        settings.model,
        window
    );
    map.insert(key, window);
}

/// Parse and remember the model window from a provider overflow error.
pub fn remember_model_context_window_from_error(settings: &ModelSettings, err: &str) {
    if let Some(window) = parse_model_context_length(err) {
        remember_model_context_window(settings, window);
    }
}

/// Best-effort parse of `maximum context length is 163840` style errors.
pub fn parse_model_context_length(err: &str) -> Option<usize> {
    let lower = err.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "this model's maximum context length is ",
        "model's maximum context length is ",
        "maximum context length is ",
        "max context length is ",
        "context length of ",
        "max_model_len is ",
        "max_model_len=",
        "max_model_len ",
    ];
    for marker in MARKERS {
        if let Some(idx) = lower.find(marker) {
            let rest = &lower[idx + marker.len()..];
            if let Some(n) = parse_leading_usize(rest) {
                if n >= 4096 {
                    return Some(n);
                }
            }
        }
    }
    None
}

fn parse_leading_usize(s: &str) -> Option<usize> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// Prompt-side compress budget: `min(configured, window - max_tokens - margin)`.
/// Unknown window is treated as the configured budget so a 160k setting on a
/// ~164k model with 32k completion still compresses before the vLLM 400.
pub fn apply_completion_reserve(configured: usize, window: Option<usize>, max_out: usize) -> usize {
    let window = window.unwrap_or(configured);
    let usable = window
        .saturating_sub(max_out)
        .saturating_sub(PROMPT_BUDGET_MARGIN_TOKENS);
    configured.min(usable.max(4096))
}

/// Compress budget used by gates and summary: configured UI budget minus
/// this round's `max_tokens`, and never above a learned model window.
pub fn compress_budget_tokens(settings: &ModelSettings) -> usize {
    let configured =
        normalize_context_budget_tokens(crate::models::effective_context_budget_tokens(settings));
    let max_out = crate::models::effective_max_tokens(settings) as usize;
    let window = remembered_model_context_window(settings);
    let budget = apply_completion_reserve(configured, window, max_out);
    if budget < configured {
        let key = format!(
            "{}:cfg={configured}:win={}:max={max_out}:prompt={budget}",
            model_window_cache_key(settings),
            window.map(|n| n.to_string()).unwrap_or_else(|| "-".into())
        );
        let mut logged = lock_mutex(logged_prompt_budget_clamps());
        if logged.insert(key) {
            log::info!(
                "context_compress: prompt budget clamped provider={} model={} configured={} window={:?} max_tokens={} margin={} prompt_budget={}",
                settings.active_provider_id,
                settings.model,
                configured,
                window,
                max_out,
                PROMPT_BUDGET_MARGIN_TOKENS,
                budget
            );
        }
    }
    budget
}

#[cfg(test)]
pub(crate) fn clear_remembered_model_windows_for_test() {
    lock_mutex(remembered_windows()).clear();
    lock_mutex(logged_prompt_budget_clamps()).clear();
}

/// Decision for soft/hard compression triggers.
#[derive(Debug, Clone, Copy)]
pub struct CompressGateDecision {
    pub should_trigger: bool,
    pub total: usize,
    pub prefix: usize,
    pub ratio: f64,
    pub split: usize,
    pub threshold: usize,
    pub payload_est: usize,
    pub api_prompt: Option<u32>,
    pub gate_source: &'static str,
}

/// Whether to compress: total over threshold, and (when estimating locally)
/// compressible prefix ratio ≥ 30%.
pub fn evaluate_compress_gate(
    history: &[ChatMessage],
    reported_prompt_tokens: Option<u32>,
    budget_tokens: usize,
    keep_users: usize,
    soft: bool,
) -> CompressGateDecision {
    let (total, payload_est, api_prompt, gate_source) =
        compression_gate_tokens(history, reported_prompt_tokens);
    let threshold = if soft {
        precompress_gate_threshold(budget_tokens)
    } else {
        budget_tokens
    };
    let using_api = api_prompt.is_some();
    let (split, prefix, ratio) = if using_api {
        // Skip the payload walk. A last real user after index 0 means there is
        // an older prefix to summarize; actual split is computed only if we compress.
        let split = crate::message_context::find_last_context_user_index(history).unwrap_or(0);
        (split, 0usize, 1.0)
    } else {
        let split = find_summary_split(history, budget_tokens, keep_users.max(1), false);
        let prefix = if split > 0 {
            estimate_message_payload_tokens(&history[..split])
        } else {
            0
        };
        let ratio = if payload_est == 0 {
            0.0
        } else {
            prefix as f64 / payload_est as f64
        };
        (split, prefix, ratio)
    };
    let ratio_ok = using_api || ratio >= COMPRESSIBLE_MIN_RATIO;
    let should_trigger = split > 0 && total > threshold && ratio_ok;
    CompressGateDecision {
        should_trigger,
        total,
        prefix,
        ratio,
        split,
        threshold,
        payload_est,
        api_prompt,
        gate_source,
    }
}

/// Isolated sub-agent loops cannot use the lead precompress queue (`conversation_id`).
/// Returns `Some(false)` at the hard budget, `Some(true)` at the 80% soft gate, `None` to skip.
pub fn sub_agent_between_round_compress_soft(
    history: &[ChatMessage],
    reported_prompt_tokens: Option<u32>,
    budget_tokens: usize,
    keep_users: usize,
) -> Option<bool> {
    let hard = plan_compression(
        history,
        reported_prompt_tokens,
        budget_tokens,
        keep_users,
        false,
        false,
        false,
    );
    if !matches!(hard, CompressionPlan::Skip) {
        return Some(false);
    }
    let soft = plan_compression(
        history,
        reported_prompt_tokens,
        budget_tokens,
        keep_users,
        true,
        false,
        false,
    );
    if !matches!(soft, CompressionPlan::Skip) {
        return Some(true);
    }
    None
}

/// Soft-threshold helper for spawn checks (same rules as soft `plan_compression`).
pub fn should_precompress_history(
    history: &[ChatMessage],
    reported_prompt_tokens: Option<u32>,
    budget_tokens: usize,
    keep_users: usize,
) -> bool {
    !matches!(
        plan_compression(
            history,
            reported_prompt_tokens,
            budget_tokens,
            keep_users,
            true,
            false,
            false,
        ),
        CompressionPlan::Skip
    )
}

/// True when provider error looks like context / prompt too large.
pub fn is_context_overflow_error(err: &anyhow::Error) -> bool {
    let s = err.to_string().to_ascii_lowercase();
    s.contains("context_length_exceeded")
        || s.contains("maximum context length")
        || s.contains("context length exceeded")
        || s.contains("prompt is too long")
        || s.contains("prompt too long")
        || s.contains("too many tokens")
        || s.contains("max tokens") && s.contains("input")
        || s.contains("range of input is too long")
        || s.contains("input is too long")
        || s.contains("token limit")
        || s.contains("exceeds the model")
        || s.contains("exceed context")
        || s.contains("context window")
        || s.contains("context size")
        || s.contains("reduce the length")
        || s.contains("request entity too large")
        || s.contains("payload too large")
        || s.contains("http 413")
        || s.contains("exceeds the limit")
}

pub(crate) fn is_cjk_dense_rune(ch: char) -> bool {
    matches!(
        ch,
        '\u{3000}'..='\u{303F}' // CJK symbols / punctuation
            | '\u{3400}'..='\u{4DBF}' // CJK Unified Ext A
            | '\u{4E00}'..='\u{9FFF}' // CJK Unified
            | '\u{3040}'..='\u{309F}' // Hiragana
            | '\u{30A0}'..='\u{30FF}' // Katakana
            | '\u{FF00}'..='\u{FFEF}' // Halfwidth / Fullwidth forms
    )
}

/// Heuristic text token estimate (matches `scripts/compare_prompt_sizes.py` / `est_tokens`).
/// CJK-dense runes ≈ 1.5 chars/token; other runes ≈ 4 chars/token.
pub fn estimate_text_tokens_heuristic(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let mut cjk_dense = 0usize;
    let mut other = 0usize;
    for ch in text.chars() {
        if is_cjk_dense_rune(ch) {
            cjk_dense += 1;
        } else {
            other += 1;
        }
    }
    (cjk_dense * 2 / 3) + (other / 4)
}

/// Qwen3.x 1080p full-frame vision tokens (see `scripts/count_prompt_tokens.py`).
pub(crate) const EST_IMAGE_TOKENS_PER_SLOT: usize = 2042;

/// UI `contextBudgetTokens` is stored as tokens; floor avoids accidental zero budget.
pub fn normalize_context_budget_tokens(budget_tokens: u32) -> usize {
    budget_tokens.max(4096) as usize
}

/// Rough payload size for compression gating (text heuristic + attached vision slots).
pub fn estimate_one_message_payload_tokens(m: &ChatMessage) -> usize {
    if !crate::message_context::is_context_included(m) {
        return 0;
    }
    let mut n = 0usize;
    n += estimate_text_tokens_heuristic(&m.content);
    if let Some(r) = &m.reasoning {
        n += estimate_text_tokens_heuristic(r);
    }
    if let Some(err) = &m.error_message {
        n += estimate_text_tokens_heuristic(err);
    }
    if let Some(tcs) = &m.tool_calls {
        for t in tcs {
            n += estimate_text_tokens_heuristic(&t.id);
            n += estimate_text_tokens_heuristic(&t.name);
            n += estimate_text_tokens_heuristic(&t.arguments);
            if let Some(res) = &t.result {
                n += estimate_text_tokens_heuristic(res);
            }
            if let Some(err) = &t.error {
                n += estimate_text_tokens_heuristic(err);
            }
        }
    }
    if let Some(id) = &m.tool_call_id {
        n += estimate_text_tokens_heuristic(id);
    }
    if let Some(imgs) = &m.images_base64 {
        n += imgs.len() * EST_IMAGE_TOKENS_PER_SLOT;
    }
    n
}

/// Rough payload size for compression gating (text heuristic + attached vision slots).
pub fn estimate_message_payload_tokens(msgs: &[ChatMessage]) -> usize {
    msgs.iter().map(estimate_one_message_payload_tokens).sum()
}

/// Compression gate tokens.
/// Prefer the last provider `prompt_tokens`, but never ignore a larger
/// local payload estimate (tool rounds can grow history after usage).
pub fn compression_gate_tokens(
    history: &[ChatMessage],
    reported_prompt_tokens: Option<u32>,
) -> (usize, usize, Option<u32>, &'static str) {
    let payload_est = estimate_message_payload_tokens(history);
    if let Some(reported) = reported_prompt_tokens.filter(|&t| t > 0) {
        let reported_n = reported as usize;
        if payload_est > reported_n {
            return (payload_est, payload_est, Some(reported), "payload_est");
        }
        return (reported_n, payload_est, Some(reported), "api_prompt");
    }
    (payload_est, payload_est, None, "payload_est")
}

/// Payload token share of `msgs[start..]`.
pub fn suffix_token_share(msgs: &[ChatMessage], start: usize) -> f64 {
    if msgs.is_empty() {
        return 0.0;
    }
    let total = estimate_message_payload_tokens(msgs);
    if total == 0 {
        return 0.0;
    }
    let start = start.min(msgs.len());
    let part = estimate_message_payload_tokens(&msgs[start..]);
    part as f64 / total as f64
}

/// Smallest index `i` such that `suffix_token_share(msgs, i) >= share`.
/// Always leaves at least one droppable message when `msgs.len() > 1`.
pub fn find_suffix_start_for_token_share(msgs: &[ChatMessage], share: f64) -> usize {
    if msgs.is_empty() {
        return 0;
    }
    let n = msgs.len();
    if n == 1 {
        return 0;
    }
    let weights: Vec<usize> = msgs
        .iter()
        .map(estimate_one_message_payload_tokens)
        .collect();
    let total: usize = weights.iter().sum();
    if total == 0 {
        return 1;
    }
    let need = ((total as f64) * share).ceil().max(1.0) as usize;
    let mut acc = 0usize;
    let mut i = n;
    while i > 1 {
        i -= 1;
        acc += weights[i];
        if acc >= need {
            return i;
        }
    }
    1
}

pub fn tail_token_ratio(overflow: bool) -> f64 {
    if overflow {
        OVERFLOW_TAIL_TOKEN_RATIO
    } else {
        TAIL_TOKEN_RATIO
    }
}

/// Index where the protected verbatim tail starts (newest ~20% of payload tokens).
pub fn find_tail_start(msgs: &[ChatMessage], overflow: bool) -> usize {
    if msgs.is_empty() {
        return 0;
    }
    let cut = find_suffix_start_for_token_share(msgs, tail_token_ratio(overflow));
    align_split_away_from_tool_group(msgs, cut)
}

/// Share of working-set **payload tokens** from the latest real user through the end.
pub fn current_turn_token_share(msgs: &[ChatMessage]) -> f64 {
    let Some(last_user) = crate::message_context::find_last_context_user_index(msgs) else {
        return 0.0;
    };
    suffix_token_share(msgs, last_user)
}

pub fn should_use_in_run_compression(msgs: &[ChatMessage]) -> bool {
    current_turn_token_share(msgs) >= IN_RUN_TURN_TOKEN_RATIO
}

/// Drop `[drop_start, tail_start)`; keep the latest user (`drop_start - 1`) and the token tail.
/// `tail_start` is aligned so an assistant + its `role: tool` rows stay on one side.
pub fn find_in_run_drop_range(
    msgs: &[ChatMessage],
    _budget_tokens: usize,
    overflow: bool,
) -> Option<(usize, usize)> {
    if msgs.len() < 3 {
        return None;
    }
    let last_user = crate::message_context::find_last_context_user_index(msgs)?;
    let tail_start = find_tail_start(msgs, overflow);
    let mut drop_start = last_user.saturating_add(1);
    while drop_start < tail_start && matches!(msgs[drop_start].role, Role::Tool) {
        drop_start += 1;
    }
    if drop_start >= tail_start {
        return None;
    }
    Some((drop_start, tail_start))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionPlan {
    Skip,
    Prefix {
        split: usize,
    },
    InRun {
        drop_start: usize,
        tail_start: usize,
    },
}

/// Choose prefix vs in-run compression once the token gate is crossed.
pub fn plan_compression(
    history: &[ChatMessage],
    reported_prompt_tokens: Option<u32>,
    budget_tokens: usize,
    keep_users: usize,
    soft: bool,
    overflow: bool,
    force: bool,
) -> CompressionPlan {
    let gate = evaluate_compress_gate(
        history,
        reported_prompt_tokens,
        budget_tokens,
        keep_users,
        soft,
    );
    let over_budget = force || overflow || gate.total > gate.threshold;
    if !over_budget {
        return CompressionPlan::Skip;
    }
    if should_use_in_run_compression(history) {
        if let Some((drop_start, tail_start)) =
            find_in_run_drop_range(history, budget_tokens, overflow)
        {
            return CompressionPlan::InRun {
                drop_start,
                tail_start,
            };
        }
        log::info!(
            "context_compress: in-run preferred but drop window empty messages={} share={:.3}",
            history.len(),
            current_turn_token_share(history)
        );
    }
    let split = find_summary_split(history, budget_tokens, keep_users.max(1), overflow);
    if split == 0 {
        return CompressionPlan::Skip;
    }
    if !force && !overflow && !gate.should_trigger {
        return CompressionPlan::Skip;
    }
    CompressionPlan::Prefix { split }
}

/// Summarize `[..split]`; keep `[split..]` verbatim.
///
/// Token-based tail (~20% of payload tokens). The latest real user turn is never
/// summarized (it may sit before the tail when the current turn is huge).
/// Fixed "keep N users" is no longer a floor — a giant previous turn can be
/// compressed even if it would have fallen inside keep-3/keep-6.
/// Live tool rows in the keep window are never shortened.
pub fn find_summary_split(
    msgs: &[ChatMessage],
    _budget_tokens: usize,
    _preferred_keep_users: usize,
    overflow: bool,
) -> usize {
    if msgs.is_empty() {
        return 0;
    }
    let tail_start = find_tail_start(msgs, overflow);
    let last_user = crate::message_context::find_last_context_user_index(msgs).unwrap_or(0);
    if last_user < tail_start {
        last_user
    } else {
        tail_start
    }
}

/// Keep window starts at `split`. Never leave an assistant in the drop window
/// and its tool result in the keep window (or the reverse).
///
/// Token or count cuts can land on the **first** tool row after the owning
/// assistant. Walking only `msgs[split - 1]` misses that case: the previous
/// row is the assistant, so the cut stayed on the tool and produced an
/// orphan `role: tool` after the summary.
pub(crate) fn align_split_away_from_tool_group(msgs: &[ChatMessage], split: usize) -> usize {
    if split == 0 || split >= msgs.len() {
        return split;
    }
    if matches!(msgs[split].role, Role::Tool) {
        let mut i = split;
        while i > 0 && matches!(msgs[i].role, Role::Tool) {
            i -= 1;
        }
        if matches!(msgs[i].role, Role::Assistant) {
            return i;
        }
        return split;
    }
    let mut i = split;
    while i > 0 && matches!(msgs[i - 1].role, Role::Tool) {
        i -= 1;
    }
    if i > 0 && i < split && matches!(msgs[i - 1].role, Role::Assistant) {
        return i - 1;
    }
    split
}
