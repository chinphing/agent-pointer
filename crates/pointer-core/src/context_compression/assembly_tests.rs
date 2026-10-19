//! Tests for Codex-style assembly reuse (compaction on the live prompt).

use super::assembly::*;
use super::budget::*;
use super::run::{assembly_window_limit, fit_assembly_drop_end, resolve_assembly_reuse};
use super::summary::*;
use super::types::{CompressionScope, CompressionUiContext};
use crate::message_context::LlmHistoryScope;
use crate::models::{ChatMessage, ModelSettings, Role, SystemPromptSections};

fn msg(id: &str, role: Role, content: &str) -> ChatMessage {
    ChatMessage {
        turn_elapsed_ms: None,
        id: id.into(),
        role,
        content: content.into(),
        status: "done".into(),
        created_at: 0,
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
    }
}

fn user(id: &str, content: &str) -> ChatMessage {
    msg(id, Role::User, content)
}

fn system_sections() -> SystemPromptSections {
    SystemPromptSections {
        cacheable: vec!["STABLE SYSTEM BLOCK".into()],
        dynamic: Vec::new(),
    }
}

fn snapshot(model: &str, last_message_id: &str) -> SessionPromptSnapshot {
    SessionPromptSnapshot {
        system: system_sections(),
        native_tools: vec![serde_json::json!({"type": "function"})],
        history_scope: LlmHistoryScope::Lead,
        provider_id: "p".into(),
        model: model.into(),
        last_message_id: last_message_id.into(),
    }
}

/// Normalize a wire row for token-wise comparison: drop `cache_control` markers
/// (metadata, not prompt tokens) and collapse the single-text-part array shape
/// that marker attachment introduces back to a plain string.
fn normalize_for_compare(value: &serde_json::Value) -> serde_json::Value {
    let mut value = value.clone();
    if let Some(parts) = value.get_mut("content").and_then(|c| c.as_array_mut()) {
        for part in parts.iter_mut() {
            if let Some(obj) = part.as_object_mut() {
                obj.remove("cache_control");
            }
        }
        let single_text = parts.len() == 1
            && parts[0].get("type").and_then(|t| t.as_str()) == Some("text")
            && parts[0].as_object().map(|o| o.len()) == Some(2);
        if single_text {
            if let Some(text) = parts[0].get("text").and_then(|t| t.as_str()) {
                value["content"] = serde_json::Value::String(text.to_string());
            }
        }
    }
    value
}

/// The core claim: the compaction request rebuilt from the session assembly is
/// byte-identical to the session request up to the truncation point. This is
/// what lets the provider prefix cache serve the whole summarized region.
#[test]
fn assembly_wire_prefix_is_byte_identical_before_truncation() {
    let history = vec![
        user("u1", "first ask"),
        msg("a1", Role::Assistant, "working"),
        user("u2", "second ask"),
        msg("a2", Role::Assistant, "more work"),
    ];
    let system = system_sections();
    let scope = LlmHistoryScope::Lead;
    let full =
        crate::models::make_openai_messages_with_inject(&history, &[], &system, false, true, false, scope);
    let drop_end = 3;
    let prefix = crate::models::make_openai_messages_with_inject(
        &history[..drop_end],
        &[],
        &system,
        false,
        true,
        false,
        scope,
    );

    assert_eq!(prefix.len(), full.len() - 1, "system + one fewer history row");
    for i in 0..prefix.len() - 1 {
        assert_eq!(prefix[i], full[i], "row {i} must be byte-identical");
    }
    // The truncated request re-uses the breakpoint for its own last row; the
    // session put it on the final row. Same tokens, different marker position.
    assert_eq!(
        normalize_for_compare(&prefix[prefix.len() - 1]),
        normalize_for_compare(&full[prefix.len() - 1]),
        "last summarized row must match the session row token-wise"
    );
}

/// The compaction instruction travels as the trailing row (session-style
/// `injected_tail`), so nothing is inserted before the reused prefix.
#[test]
fn assembly_instruction_is_trailing_and_history_keeps_breakpoint() {
    let history = vec![user("u1", "ask"), msg("a1", Role::Assistant, "work")];
    let system = system_sections();
    let instruction = user("instr", "CONTEXT CHECKPOINT COMPACTION");
    let wire = crate::models::make_openai_messages_with_inject(
        &history,
        std::slice::from_ref(&instruction),
        &system,
        false,
        true,
        false,
        LlmHistoryScope::Lead,
    );

    let last = wire.last().expect("instruction row");
    assert_eq!(last["role"], "user");
    assert!(last["content"]
        .as_str()
        .expect("instruction is plain text")
        .contains("CONTEXT CHECKPOINT COMPACTION"));
    assert!(
        last.get("content")
            .and_then(|c| c.as_array())
            .is_none(),
        "instruction must not carry a cache marker"
    );

    let before = &wire[wire.len() - 2];
    assert_eq!(before["role"], "assistant");
    assert!(
        before["content"]
            .as_array()
            .and_then(|parts| parts.first())
            .and_then(|part| part.get("cache_control"))
            .is_some(),
        "breakpoint stays on the last history row, before the instruction"
    );
}

#[test]
fn snapshot_registry_is_scoped_by_agent_instance() {
    remember_session_prompt("conv-scope", None, snapshot("lead-model", "m-lead"));
    remember_session_prompt(
        "conv-scope",
        Some("sub-1"),
        snapshot("sub-model", "m-sub"),
    );

    assert_eq!(
        session_prompt_for("conv-scope", None).map(|s| s.model),
        Some("lead-model".to_string())
    );
    assert_eq!(
        session_prompt_for("conv-scope", Some("sub-1")).map(|s| s.model),
        Some("sub-model".to_string())
    );
    assert!(session_prompt_for("conv-scope", Some("sub-2")).is_none());
    assert!(session_prompt_for("", None).is_none());
}

#[test]
fn snapshot_covers_history_detects_stale_round() {
    let snap = snapshot("m", "u2");
    let live = vec![user("u1", "a"), user("u2", "b"), user("u3", "c")];
    assert!(snap.covers_history(&live));

    // History rewritten by an earlier compression no longer contains the row
    // the snapshot was taken from.
    let rewritten = vec![user("summary", "checkpoint"), user("u3", "c")];
    assert!(!snap.covers_history(&rewritten));
}

#[test]
fn assembly_window_trims_from_the_end_until_it_fits() {
    let settings = ModelSettings::default();
    let budget = compress_budget_tokens(&settings);
    // Each message is ~1/8 of the prompt budget, so 12 of them cannot fit.
    let chunk = "x".repeat((budget / 8).max(64) * 4);
    let history: Vec<ChatMessage> = (0..12)
        .map(|i| user(&format!("u{i}"), &chunk))
        .collect();

    let fitted = fit_assembly_drop_end(&history, 0, history.len(), &settings, 0)
        .expect("a smaller window must still fit");
    assert!(fitted > 0, "keep the oldest messages");
    assert!(fitted < history.len(), "oversized window must be trimmed");

    let kept = estimate_message_payload_tokens(&history[..fitted]);
    let out = summary_max_tokens_requested(compute_summary_max_tokens(kept)) as usize;
    // `assembly_window_limit` is already net of the summary output, so the
    // fitted window is compared against the limit itself (adding `out` here
    // would double-count the reserve).
    let limit = assembly_window_limit(&settings, out);
    assert!(
        kept <= limit,
        "fitted window {kept} must fit the window limit {limit} (output {out} already reserved)"
    );
    assert_eq!(
        history[fitted].id,
        format!("u{fitted}"),
        "the walk drops the newest messages of the window, keeping a strict prefix"
    );
}

#[test]
fn assembly_window_refuses_when_nothing_fits() {
    let settings = ModelSettings::default();
    let budget = compress_budget_tokens(&settings);
    let chunk = "x".repeat((budget * 4).max(64));
    let history = vec![user("u1", &chunk), user("u2", &chunk)];
    assert!(
        fit_assembly_drop_end(&history, 0, history.len(), &settings, 0).is_none(),
        "when even one message cannot fit, the legacy compact prompt takes over"
    );
}

#[test]
fn assembly_window_uses_the_model_window_not_the_session_reserve() {
    let _window_lock = crate::test_support::remembered_model_window_lock();
    clear_remembered_model_windows_for_test();
    let mut s = crate::models::sample_settings();
    s.model = "assembly-window-base".into();
    s.context_budget_tokens = 262_144;
    s.max_tokens = 64_000;
    s.providers[0].models.push("assembly-window-base".into());
    s.providers[0].context_budget_tokens = Some(262_144);
    s.providers[0].max_tokens = Some(64_000);
    remember_model_context_window(&s, 262_144);

    // The compaction call reserves its own output (~18k at this size), not the
    // session's 64k answer: 262144 - 18000 - 64.
    assert_eq!(assembly_window_limit(&s, 18_000), 244_080);
    assert!(
        assembly_window_limit(&s, 18_000) > compress_budget_tokens(&s),
        "the session answer reserve must not shrink the compaction window"
    );

    // ~200k payload tokens of prefix: over the old `budget - output` ceiling
    // (198080 - 12000) but under the window limit, so it must not be trimmed.
    let chunk = "x".repeat(200_000 * 4);
    let history = vec![user("u1", &chunk), user("u2", "tail")];
    assert_eq!(
        fit_assembly_drop_end(&history, 0, history.len(), &s, 0),
        Some(history.len())
    );
}

#[test]
fn assembly_reuse_refuses_overflow_recovery() {
    let settings = ModelSettings::default();
    let history = vec![user("u1", "ask")];
    remember_session_prompt("conv-overflow", None, snapshot("m", "u1"));
    let ui = CompressionUiContext::default();
    assert!(
        resolve_assembly_reuse(&history, &settings, "conv-overflow", &ui, true).is_none(),
        "overflow recovery must keep the legacy prompt (the session prompt did not fit)"
    );
}

#[test]
fn assembly_reuse_accepts_live_snapshot_with_matching_provider() {
    let settings = ModelSettings::default();
    let history = vec![user("u1", "ask")];
    let mut live = snapshot(&settings.model, "u1");
    live.provider_id = settings.active_provider_id.clone();
    remember_session_prompt("conv-live", None, live);
    let ui = CompressionUiContext::default();
    assert!(
        resolve_assembly_reuse(&history, &settings, "conv-live", &ui, false).is_some(),
        "matching provider/model + live history should reuse the session prefix"
    );
}

#[test]
fn assembly_reuse_refuses_provider_switch() {
    let settings = ModelSettings::default();
    let history = vec![user("u1", "ask")];
    let mut switched = snapshot("some-other-model", "u1");
    switched.provider_id = settings.active_provider_id.clone();
    remember_session_prompt("conv-switch", None, switched);
    let ui = CompressionUiContext::default();
    assert!(
        resolve_assembly_reuse(&history, &settings, "conv-switch", &ui, false).is_none(),
        "a different model must not reuse the snapshot"
    );
}

#[test]
fn compaction_instruction_keeps_the_summary_contract() {
    let ui = CompressionUiContext::default();
    let text = build_compaction_instruction_message_with_language(
        &ui,
        false,
        false,
        SummaryBodyLanguage::Chinese,
        4_000,
    );
    assert!(text.contains("CONTEXT CHECKPOINT COMPACTION"));
    assert!(text.contains("do not answer"));
    assert!(text.contains("[REDACTED]"));
    assert!(text.contains("HARD CEILING"));
    assert!(text.contains("## Goal"));
    assert!(text.contains("## Progress"));
    assert!(text.contains("## State"));
    assert!(text.contains("## Open"));
    assert!(text.contains("Chinese"), "language rule must survive");
}

#[test]
fn compaction_instruction_marks_sub_agent_scope() {
    let ui = CompressionUiContext {
        scope: CompressionScope::SubAgent,
        sub_agent_id: Some("explore".into()),
        ..CompressionUiContext::default()
    };
    let text = build_compaction_instruction_message_with_language(
        &ui,
        true,
        true,
        SummaryBodyLanguage::English,
        2_000,
    );
    assert!(text.contains("Sub-agent scope"));
    assert!(text.contains("read-only explore worker"));
    assert!(text.contains("## Next"));
}
