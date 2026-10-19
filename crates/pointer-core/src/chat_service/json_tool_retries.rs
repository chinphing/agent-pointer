use crate::models::{ChatMessage, Role, StreamEvent};

use super::emit::emit;
use super::util::{new_id, now_ms};
use super::StreamTx;

pub(crate) fn output_length_retry_supplement(max_tokens: u32, finish_reason: &str) -> String {
    format!(
        "\n\n【输出长度】本回合因 **输出 token 上限** 被截断（finish_reason={finish_reason}，配置 max_tokens≈{max_tokens}）。\
         请**缩小**本回合 payload：拆分 `file_edit` / `file_write`、缩短单次参数体积，分多轮完成。"
    )
}

/// Minimum output budget when a reasoning model spent the whole allowance on
/// thinking and produced no visible reply. Retrying with the original (small)
/// budget would reproduce the same stop, so the retry must get real headroom.
pub(crate) const REASONING_LENGTH_OUTPUT_FLOOR: u32 = 16_384;

/// Upper bound used when the provider/model declares no output limit of its own.
pub(crate) const REASONING_LENGTH_OUTPUT_CEILING: u32 = 65_536;

/// A `finish_reason == "length"` round that produced **no visible output**
/// (no content, no tool call) but **did** emit reasoning.
///
/// This is *not* a payload truncation: nothing observable was cut off. The
/// model burned its output budget on chain-of-thought. Re-sending the identical
/// request at the same budget is futile — the caller must either raise the
/// budget or change the ask.
pub(crate) fn is_reasoning_only_length_stop(
    finish_reason: &str,
    has_visible_output: bool,
    has_reasoning: bool,
) -> bool {
    finish_reason == "length" && !has_visible_output && has_reasoning
}

/// Upper bound for the escalated output budget, given what the provider/model
/// declares and the user's own output setting.
///
/// `declared_max_tokens` cannot tell a real model cap from the migrated copy of
/// the user's setting (`ensure_provider_generation_defaults` fills
/// `provider.max_tokens` with the global value when unset). When the two are
/// equal we treat it as a soft preference and allow growth to the hard ceiling;
/// otherwise the declared value is respected as a cap.
pub(crate) fn reasoning_output_ceiling(declared: Option<u32>, user_max_tokens: u32) -> u32 {
    match declared {
        Some(n) if n == user_max_tokens => REASONING_LENGTH_OUTPUT_CEILING.max(n),
        Some(n) => n,
        None => REASONING_LENGTH_OUTPUT_CEILING,
    }
}

/// Next output budget after a reasoning-only length stop: grow geometrically
/// from `current` toward [`REASONING_LENGTH_OUTPUT_FLOOR`], never exceeding
/// `ceiling` (see [`reasoning_output_ceiling`]).
///
/// Returns `current` unchanged when it is already at the ceiling, so callers
/// can detect that escalation is impossible.
pub(crate) fn escalated_reasoning_output_budget(current: u32, ceiling: u32) -> u32 {
    let ceiling = ceiling.max(current);
    current
        .saturating_mul(2)
        .max(REASONING_LENGTH_OUTPUT_FLOOR)
        .min(ceiling)
}

/// Next output budget for a `finish_reason == "length"` retry, for **both**
/// truncation kinds: a reasoning-only stop that burned the budget on thinking,
/// and a visible truncation where content / tool arguments were cut off.
///
/// Composes [`reasoning_output_ceiling`] with
/// [`escalated_reasoning_output_budget`] so both call sites share one ladder and
/// cannot forget the provider cap. Returns `current` unchanged when it is already
/// at the ceiling.
pub(crate) fn escalated_length_output_budget(
    current: u32,
    declared_max_tokens: Option<u32>,
    user_max_tokens: u32,
) -> u32 {
    let ceiling = reasoning_output_ceiling(declared_max_tokens, user_max_tokens);
    escalated_reasoning_output_budget(current, ceiling)
}

/// Retry hint for a reasoning-only length stop: the budget was raised, so the
/// model should converge on the answer instead of "shrinking the payload".
pub(crate) fn reasoning_length_retry_supplement(
    previous_max_tokens: u32,
    next_max_tokens: u32,
) -> String {
    format!(
        "\n\n【输出长度】上一回合的思考（reasoning）占满了输出预算（max_tokens≈{previous_max_tokens}），\
         因此没有产出可见回复或工具调用。本回合输出预算已提高到 ≈{next_max_tokens}。\
         请尽快收敛思考，直接给出最终回复或发起工具调用。"
    )
}

pub(crate) fn push_injected_format_retry_turn(
    stream: &StreamTx,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    hint: String,
) {
    let retry_id = new_id("fmt_retry");
    emit(
        stream,
        StreamEvent::InjectedUserMessage {
            conversation_id: conversation_id.to_string(),
            message_id: retry_id.clone(),
            content: hint.clone(),
            attachments: None,
            ui_bindings: None,
        },
    );
    history.push(ChatMessage {
        turn_elapsed_ms: None,
        id: retry_id.clone(),
        role: Role::User,
        content: hint.clone(),
        status: "done".into(),
        created_at: now_ms(),
        tool_calls: None,
        tool_call_id: None,
        tool_name: None,
        error_message: None,
        reasoning: None,
        thoughts: None,
        headline: None,
        raw_content: None,
        tool_raw_output: None,
        agent_id: None,
        agent_instance_id: None,
        agent_name: None,
        agent_trace: None,
        image_slot_labels: None,
        images_base64: None,
        computer_round_screen_rel_path: None,
        ui_bindings: None,
        context_state: None,
        attachments: None,
        anchor_message_id: None,
        trace_id: None,
        task_id: None,
        spawn_depth: None,
        agent_chain: None,
    });
    super::conversation_persist::upsert_message(
        conversation_id,
        history.last().expect("injected retry turn"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_only_length_is_not_visible_truncation() {
        // length + no visible output + reasoning → reasoning-only stop.
        assert!(is_reasoning_only_length_stop("length", false, true));
        // length + visible output → real truncation (even with reasoning present).
        assert!(!is_reasoning_only_length_stop("length", true, true));
        assert!(!is_reasoning_only_length_stop("length", true, false));
        // length + no visible output + no reasoning → plain empty response path.
        assert!(!is_reasoning_only_length_stop("length", false, false));
        // normal stops are never truncation.
        assert!(!is_reasoning_only_length_stop("stop", false, true));
        assert!(!is_reasoning_only_length_stop("tool_calls", false, true));
    }

    #[test]
    fn escalation_grows_to_floor_and_doubles() {
        // Default 2048 (CLI) jumps to the floor.
        assert_eq!(escalated_reasoning_output_budget(2048, 65_536), 16_384);
        // Then doubles.
        assert_eq!(escalated_reasoning_output_budget(16_384, 65_536), 32_768);
        assert_eq!(escalated_reasoning_output_budget(32_768, 65_536), 65_536);
    }

    #[test]
    fn length_retry_budget_escalates_for_any_truncation_kind() {
        // New product default (16384) still has headroom: 16384 → 32768 → 65536.
        assert_eq!(escalated_length_output_budget(16_384, None, 16_384), 32_768);
        assert_eq!(escalated_length_output_budget(32_768, None, 16_384), 65_536);
        // Bounded: at the ceiling it stays put (caller can detect a no-op).
        assert_eq!(escalated_length_output_budget(65_536, None, 16_384), 65_536);
        // A real provider cap distinct from the user setting is respected.
        assert_eq!(
            escalated_length_output_budget(16_384, Some(20_000), 16_384),
            20_000
        );
        // A migrated copy of the user setting is soft → the hard ceiling applies.
        assert_eq!(
            escalated_length_output_budget(16_384, Some(16_384), 16_384),
            32_768
        );
    }

    #[test]
    fn ceiling_treats_migrated_default_as_soft_but_respects_real_caps() {
        // CLI case: provider value is the migrated copy of the user setting.
        assert_eq!(reasoning_output_ceiling(Some(2048), 2048), 65_536);
        // A provider/model cap distinct from the user setting is respected.
        assert_eq!(reasoning_output_ceiling(Some(8_192), 2048), 8_192);
        // No declaration → hard ceiling.
        assert_eq!(reasoning_output_ceiling(None, 2048), 65_536);
        // Declared cap larger than the hard ceiling wins.
        assert_eq!(reasoning_output_ceiling(Some(100_000), 2048), 100_000);
    }

    #[test]
    fn escalation_respects_ceiling_and_never_shrinks() {
        // Declared provider ceiling caps growth.
        assert_eq!(escalated_reasoning_output_budget(2048, 8_192), 8_192);
        // Already at the ceiling → unchanged (caller can detect no-op).
        assert_eq!(escalated_reasoning_output_budget(8_192, 8_192), 8_192);
        // A ceiling below the current budget must not shrink the request.
        assert_eq!(escalated_reasoning_output_budget(16_384, 8_192), 16_384);
        // Ceiling below the floor but above current clamps to ceiling.
        assert_eq!(escalated_reasoning_output_budget(2048, 4_096), 4_096);
    }

    #[test]
    fn reasoning_supplement_reports_both_budgets() {
        let hint = reasoning_length_retry_supplement(2048, 16_384);
        assert!(hint.contains("2048"));
        assert!(hint.contains("16384"));
        // Must not tell the model to shrink a payload that was never produced.
        assert!(!hint.contains("缩小"));
    }
}
