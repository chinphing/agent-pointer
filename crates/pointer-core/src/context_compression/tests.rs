use super::budget::*;
use super::precompress::*;
use super::summary::*;
use super::*;
use crate::agent_instance_scope::AgentInstanceScope;
use crate::message_context::find_split_at_user_boundary;
use crate::models::{ChatMessage, Role};

fn u(s: &str) -> ChatMessage {
    ChatMessage {
        id: "u".into(),
        role: Role::User,
        content: s.into(),
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
    }
}

#[test]
fn split_keeps_last_n_users() {
    let msgs = vec![u("a"), u("b"), u("c")];
    assert_eq!(find_split_at_user_boundary(&msgs, 1), 2);
    assert_eq!(find_split_at_user_boundary(&msgs, 2), 1);
    assert_eq!(find_split_at_user_boundary(&msgs, 3), 0);
}

#[test]
fn split_fewer_users_than_keep_returns_zero() {
    let msgs = vec![u("only")];
    assert_eq!(find_split_at_user_boundary(&msgs, 2), 0);
}

#[test]
fn compressed_prefix_excludes_old_summaries_and_regular_messages() {
    let mut old_summary = u(&format!("{SUMMARY_PREFIX_BUDGET}\nold summary"));
    old_summary.id = "old-summary".into();
    let mut regular = u("regular history");
    regular.id = "regular".into();
    let ids = mark_compressed_prefix_excluded(std::slice::from_mut(&mut old_summary));
    assert_eq!(ids, vec!["old-summary"]);
    assert!(!crate::message_context::is_context_included(&old_summary));

    let ids = mark_compressed_prefix_excluded(std::slice::from_mut(&mut regular));
    assert_eq!(ids, vec!["regular"]);
    assert!(!crate::message_context::is_context_included(&regular));
}

#[test]
fn sub_agent_ui_context_carries_agent_fields() {
    let ui = CompressionUiContext::sub_agent(
        AgentInstanceScope::new("test-run", "conv", "explore"),
        "msg_1",
        "explore",
        "Explore Agent",
        "task_a",
    );
    assert_eq!(ui.scope, CompressionScope::SubAgent);
    assert_eq!(ui.sub_agent_id.as_deref(), Some("explore"));
    assert_eq!(ui.task_id.as_deref(), Some("task_a"));
}

#[test]
fn user_messages_get_larger_snippet_than_assistant() {
    assert!(content_snippet_limit(&Role::User) > content_snippet_limit(&Role::Assistant));
}

#[test]
fn grep_tool_output_limit_exceeds_file_read() {
    assert!(tool_output_snippet_limit("file_grep") > tool_output_snippet_limit("file_read"));
}

#[test]
fn text_token_heuristic_matches_python_est_tokens() {
    assert_eq!(estimate_text_tokens_heuristic(""), 0);
    // 4000 ASCII → 1000 tokens (other/4)
    assert_eq!(estimate_text_tokens_heuristic(&"x".repeat(4000)), 1000);
    // 1500 CJK unified → 1000 tokens (cjk/1.5)
    assert_eq!(estimate_text_tokens_heuristic(&"中".repeat(1500)), 1000);
}

#[test]
fn summary_max_tokens_scales_with_content_and_caps_at_12k() {
    // Small content still gets the 1.5k floor.
    assert_eq!(compute_summary_max_tokens(1_000), 1_500);
    // 20k content → 4k budget (×0.20).
    assert_eq!(compute_summary_max_tokens(20_000), 4_000);
    // Huge content caps at one-shot ceiling 12k.
    assert_eq!(compute_summary_max_tokens(500_000), 12_000);
}

#[test]
fn evaluate_compress_gate_uses_api_prompt_without_payload_walk() {
    let budget = 10_000;
    let prefix_heavy = vec![u(&"old ".repeat(20_000)), u(&"keep ".repeat(500))];
    let d = evaluate_compress_gate(&prefix_heavy, Some(200_000), budget, 1, true);
    assert!(d.total > precompress_gate_threshold(budget));
    assert_eq!(d.gate_source, "api_prompt");
    assert_eq!(d.payload_est, 0);
    assert!(d.should_trigger);
    assert!(d.split > 0);
}

#[test]
fn evaluate_compress_gate_requires_prefix_ratio() {
    let budget = 10_000;
    // Keep zone huge, compressible prefix tiny → do not trigger.
    let keep_heavy = vec![
        u(&"old ".repeat(500)),     // compressible
        u(&"keep ".repeat(20_000)), // keep (newest)
    ];
    let d = evaluate_compress_gate(&keep_heavy, None, budget, 1, true);
    assert!(d.total > precompress_gate_threshold(budget));
    assert!(d.ratio < COMPRESSIBLE_MIN_RATIO);
    assert!(!d.should_trigger);

    // Prefix carries most mass → trigger.
    let prefix_heavy = vec![u(&"old ".repeat(20_000)), u(&"keep ".repeat(500))];
    let d2 = evaluate_compress_gate(&prefix_heavy, None, budget, 1, true);
    assert!(d2.total > precompress_gate_threshold(budget));
    assert!(d2.ratio >= COMPRESSIBLE_MIN_RATIO);
    assert!(d2.should_trigger);
}

#[test]
fn token_tail_split_compresses_older_turn_despite_keep_users() {
    // Three user turns: keep-3 would be split=0. Token tail (~20%) still
    // leaves the oldest (token-heavy) message outside the keep window.
    let msgs = vec![u(&"old ".repeat(40_000)), u("follow-up"), u("current")];
    assert_eq!(find_split_at_user_boundary(&msgs, 3), 0);
    let budget = 20_000;
    let split = find_summary_split(&msgs, budget, 3, false);
    assert!(
        split > 0,
        "oldest turn must be outside the token tail, split={split}"
    );
    let d = evaluate_compress_gate(&msgs, None, budget, 3, true);
    assert!(d.should_trigger);
    assert!(d.ratio >= COMPRESSIBLE_MIN_RATIO);
}

const EVEN: &str = "xxxxxxxx";

#[test]
fn token_tail_keeps_newest_twenty_percent() {
    let msgs: Vec<_> = (0..20)
        .map(|i| msg(&format!("m{i}"), Role::Assistant, EVEN))
        .collect();
    let start = find_suffix_start_for_token_share(&msgs, TAIL_TOKEN_RATIO);
    assert_eq!(start, 16);
    assert!(suffix_token_share(&msgs, start) >= TAIL_TOKEN_RATIO);
    assert!(suffix_token_share(&msgs, start + 1) < TAIL_TOKEN_RATIO);
    let overflow_start = find_suffix_start_for_token_share(&msgs, OVERFLOW_TAIL_TOKEN_RATIO);
    assert!(overflow_start >= start);
    assert_eq!(find_tail_start(&msgs, false), start);
}

#[test]
fn align_split_keeps_assistant_when_cut_lands_on_first_tool() {
    // 20 equal-size rows → 20% token tail is 4 → unaligned cut index 16,
    // which is the first tool row after its assistant.
    let mut msgs = Vec::new();
    msgs.push(msg("u0", Role::User, EVEN));
    for i in 0..14 {
        msgs.push(msg(&format!("pad{i}"), Role::Assistant, EVEN));
    }
    msgs.push(msg("a_own", Role::Assistant, EVEN));
    msgs.push(msg("t_own", Role::Tool, EVEN));
    msgs.push(msg("k0", Role::Assistant, EVEN));
    msgs.push(msg("k1", Role::Assistant, EVEN));
    msgs.push(msg("k2", Role::Assistant, EVEN));
    assert_eq!(msgs.len(), 20);
    assert_eq!(msgs[16].id, "t_own");
    assert_eq!(
        find_suffix_start_for_token_share(&msgs, TAIL_TOKEN_RATIO),
        16
    );
    let tail_start = find_tail_start(&msgs, false);
    assert_eq!(tail_start, 15);
    assert_eq!(msgs[tail_start].id, "a_own");
    let (drop_start, range_tail) =
        find_in_run_drop_range(&msgs, 8_000, false).expect("drop window");
    assert_eq!(drop_start, 1);
    assert_eq!(range_tail, 15);
    assert!(
        !matches!(msgs[range_tail].role, Role::Tool),
        "keep must not start on a tool row"
    );
}

#[test]
fn align_split_still_pulls_back_when_cut_is_mid_tool_run() {
    let mut msgs = Vec::new();
    msgs.push(msg("u0", Role::User, EVEN));
    for i in 0..13 {
        msgs.push(msg(&format!("pad{i}"), Role::Assistant, EVEN));
    }
    msgs.push(msg("a_own", Role::Assistant, EVEN));
    msgs.push(msg("t0", Role::Tool, EVEN));
    msgs.push(msg("t1", Role::Tool, EVEN));
    msgs.push(msg("k0", Role::Assistant, EVEN));
    msgs.push(msg("k1", Role::Assistant, EVEN));
    msgs.push(msg("k2", Role::Assistant, EVEN));
    assert_eq!(msgs.len(), 20);
    assert_eq!(msgs[16].id, "t1");
    let tail_start = find_tail_start(&msgs, false);
    assert_eq!(msgs[tail_start].id, "a_own");
}

#[test]
fn splice_pending_into_history_replaces_prefix_with_summary() {
    let mut old = u("old");
    old.id = "old".into();
    let mut keep = u("keep");
    keep.id = "keep".into();
    let mut summary = u("summary");
    summary.id = "sum".into();
    let mut history = vec![old.clone(), keep.clone()];
    let pending = PendingCompressionSplice {
        queue_key: "c".into(),
        conversation_id: "c".into(),
        ui: CompressionUiContext::default(),
        fingerprint_prefix_ids: vec!["old".into()],
        insert_before_message_id: "keep".into(),
        excluded_for_persist: vec![old],
        summary_msg: summary.clone(),
        preview_for_disk: String::new(),
        dropped_count: 1,
        keep_users: 1,
        apply_reason: "budget".into(),
        summary_failed: false,
    };
    assert!(splice_pending_into_history(&mut history, &pending));
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].id, "sum");
    assert_eq!(history[1].id, "keep");
}

#[test]
fn splice_pending_keeps_fallback_user_and_concluding_assistant() {
    let mut history = vec![
        msg("u_old", Role::User, "older"),
        msg("a_old", Role::Assistant, "older reply"),
        msg("u0", Role::User, "first"),
        msg("a0", Role::Assistant, "tools"),
        msg("t0", Role::Tool, "out"),
        msg("a0f", Role::Assistant, "done first"),
        msg("u1", Role::User, "second"),
        msg("a1f", Role::Assistant, "done second"),
        msg("u2", Role::User, "third"),
        msg("a2f", Role::Assistant, "done third"),
        msg("u_cur", Role::User, "current"),
    ];
    let drop_end = 10;
    let keep_ids = super::run::drop_fallback_keep_ids(&history, 0, drop_end);
    assert!(!keep_ids.contains("u_old"));
    assert!(!keep_ids.contains("a_old"));
    assert!(!keep_ids.contains("u0"));
    assert!(!keep_ids.contains("a0f"));
    assert!(keep_ids.contains("u1"));
    assert!(keep_ids.contains("a1f"));
    assert!(keep_ids.contains("u2"));
    assert!(keep_ids.contains("a2f"));
    assert!(!keep_ids.contains("a0"));
    assert!(!keep_ids.contains("t0"));

    let excluded = history[..drop_end]
        .iter()
        .filter(|m| !keep_ids.contains(&m.id))
        .cloned()
        .collect::<Vec<_>>();
    let pending = PendingCompressionSplice {
        queue_key: "c".into(),
        conversation_id: "c".into(),
        ui: CompressionUiContext::default(),
        fingerprint_prefix_ids: history[..drop_end].iter().map(|m| m.id.clone()).collect(),
        insert_before_message_id: "u1".into(),
        excluded_for_persist: excluded,
        summary_msg: msg("sum", Role::User, "summary"),
        preview_for_disk: String::new(),
        dropped_count: 6,
        keep_users: 1,
        apply_reason: "budget_drop".into(),
        summary_failed: true,
    };
    assert!(splice_pending_into_history(&mut history, &pending));
    let ids: Vec<&str> = history.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["sum", "u1", "a1f", "u2", "a2f", "u_cur"]);
}

#[test]
fn drop_fallback_skips_unconcluded_assistant_with_trailing_tools() {
    let history = vec![
        msg("u0", Role::User, "task"),
        msg("a0", Role::Assistant, "call"),
        msg("t0", Role::Tool, "out"),
        msg("keep", Role::User, "current"),
    ];
    let keep_ids = super::run::drop_fallback_keep_ids(&history, 0, 3);
    assert!(keep_ids.contains("u0"));
    assert!(!keep_ids.contains("a0"));
    assert!(!keep_ids.contains("t0"));
}

#[test]
fn drop_fallback_ignores_users_already_in_tail() {
    let history = vec![
        msg("u0", Role::User, "old"),
        msg("a0", Role::Assistant, "old reply"),
        msg("u1", Role::User, "mid"),
        msg("a1", Role::Assistant, "mid reply"),
        msg("u2", Role::User, "keep-a"),
        msg("a2", Role::Assistant, "a"),
        msg("u3", Role::User, "keep-b"),
        msg("a3", Role::Assistant, "b"),
        msg("u4", Role::User, "keep-c"),
    ];
    let keep_ids = super::run::drop_fallback_keep_ids(&history, 0, 4);
    assert!(
        keep_ids.is_empty(),
        "last 3 user turns start at u2, all in the tail: {keep_ids:?}"
    );
}

#[test]
fn in_run_summary_is_assistant_prefix_is_user() {
    let prefix = new_summary_message("body".into(), false);
    assert!(matches!(prefix.role, Role::User));
    let in_run = new_summary_message("body".into(), true);
    assert!(matches!(in_run.role, Role::Assistant));
    assert_eq!(
        in_run.reasoning.as_deref(),
        Some(COMPRESSION_SUMMARY_REASONING)
    );
    assert!(prefix.reasoning.is_none());
}

fn msg(id: &str, role: Role, content: &str) -> ChatMessage {
    let mut m = u(content);
    m.id = id.into();
    m.role = role;
    m
}

#[test]
fn current_turn_share_selects_in_run_by_tokens() {
    let mut long_turn = vec![msg("u0", Role::User, "task")];
    for i in 0..9 {
        long_turn.push(msg(&format!("a{i}"), Role::Assistant, "step"));
    }
    assert!(should_use_in_run_compression(&long_turn));
    assert!((current_turn_token_share(&long_turn) - 1.0).abs() < f64::EPSILON);

    let mixed = vec![
        msg("u0", Role::User, "old-goal-text"),
        msg("a0", Role::Assistant, "old-reply-ok"),
        msg("u1", Role::User, "new-goal-text"),
        msg("a1", Role::Assistant, "new-reply-ok"),
    ];
    assert!(current_turn_token_share(&mixed) < IN_RUN_TURN_TOKEN_RATIO);
    assert!(!should_use_in_run_compression(&mixed));
}

#[test]
fn in_run_when_current_turn_is_most_tokens_even_if_under_count_ratio() {
    // Last user at 7 → count share 12/19 ≈ 0.63. Prefix is tiny; current-turn
    // assistants hold the tokens. Newest keep rows cover the 20% token tail
    // so in-run still has a drop window.
    let mut msgs = Vec::new();
    for i in 0..7 {
        msgs.push(msg(&format!("old{i}"), Role::Assistant, EVEN));
    }
    msgs.push(msg("u_now", Role::User, EVEN));
    for i in 0..8 {
        msgs.push(msg(&format!("big{i}"), Role::Assistant, &"x".repeat(8_000)));
    }
    for i in 0..3 {
        msgs.push(msg(
            &format!("keep{i}"),
            Role::Assistant,
            &"y".repeat(24_000),
        ));
    }
    assert_eq!(msgs.len(), 19);
    let count_share = (msgs.len() - 7) as f64 / msgs.len() as f64;
    assert!(count_share < IN_RUN_TURN_TOKEN_RATIO);
    assert!(current_turn_token_share(&msgs) >= IN_RUN_TURN_TOKEN_RATIO);
    match plan_compression(&msgs, None, 8_000, 1, false, false, true) {
        CompressionPlan::InRun { drop_start, .. } => assert_eq!(drop_start, 8),
        other => panic!("expected InRun, got {other:?}"),
    }
}

#[test]
fn prefix_when_current_turn_is_many_short_rows_but_few_tokens() {
    let mut msgs = vec![
        msg("u0", Role::User, "old"),
        msg("a0", Role::Assistant, &"x".repeat(80_000)),
    ];
    msgs.push(msg("u1", Role::User, "now"));
    for i in 0..20 {
        msgs.push(msg(&format!("a{i}"), Role::Assistant, "ok"));
    }
    let last_user = 2usize;
    let count_share = (msgs.len() - last_user) as f64 / msgs.len() as f64;
    assert!(count_share > IN_RUN_TURN_TOKEN_RATIO);
    assert!(current_turn_token_share(&msgs) < IN_RUN_TURN_TOKEN_RATIO);
    assert!(!should_use_in_run_compression(&msgs));
    match plan_compression(&msgs, None, 8_000, 1, false, false, true) {
        CompressionPlan::Prefix { split } => {
            assert!(split > 0);
            assert!(split <= last_user);
        }
        other => panic!("expected Prefix, got {other:?}"),
    }
}

#[test]
fn plan_compression_in_run_when_latest_turn_dominates_tokens() {
    let mut msgs = vec![msg("u0", Role::User, "task")];
    for i in 0..12 {
        msgs.push(msg(&format!("a{i}"), Role::Assistant, &"x".repeat(8_000)));
    }
    let plan = plan_compression(&msgs, None, 8_000, 1, true, false, false);
    match plan {
        CompressionPlan::InRun {
            drop_start,
            tail_start,
        } => {
            assert_eq!(drop_start, 1);
            assert!(tail_start > drop_start);
        }
        other => panic!("expected InRun, got {other:?}"),
    }
}

#[test]
fn sub_agent_queue_key_is_isolated_from_lead() {
    let lead = "conv-1";
    let key = sub_agent_compression_queue_key(lead, "inst-a");
    assert_ne!(key, lead);
    assert!(key.starts_with("sub:"));
    assert_ne!(
        sub_agent_compression_queue_key(lead, "inst-a"),
        sub_agent_compression_queue_key(lead, "inst-b")
    );
}

#[test]
fn sub_agent_between_round_uses_soft_then_hard_gate() {
    let mut msgs = vec![msg("u0", Role::User, "task")];
    for i in 0..12 {
        msgs.push(msg(&format!("a{i}"), Role::Assistant, &"x".repeat(8_000)));
    }
    let budget = 100_000;
    assert_eq!(
        sub_agent_between_round_compress_soft(&msgs, Some(85_000), budget, 1),
        Some(true)
    );
    assert_eq!(
        sub_agent_between_round_compress_soft(&msgs, Some(110_000), budget, 1),
        Some(false)
    );
    assert_eq!(
        sub_agent_between_round_compress_soft(&msgs, Some(1_000), budget, 1),
        None
    );
}

#[test]
fn splice_pending_in_run_keeps_last_user() {
    let mut history = vec![
        msg("u0", Role::User, "task"),
        msg("a0", Role::Assistant, "mid"),
        msg("a1", Role::Assistant, "tail"),
    ];
    let pending = PendingCompressionSplice {
        queue_key: "c".into(),
        conversation_id: "c".into(),
        ui: CompressionUiContext::default(),
        fingerprint_prefix_ids: vec!["a0".into()],
        insert_before_message_id: "a1".into(),
        excluded_for_persist: vec![msg("a0", Role::Assistant, "mid")],
        summary_msg: msg("sum", Role::User, "summary"),
        preview_for_disk: String::new(),
        dropped_count: 1,
        keep_users: 1,
        apply_reason: "in_run".into(),
        summary_failed: false,
    };
    assert!(splice_pending_into_history(&mut history, &pending));
    let ids: Vec<&str> = history.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["u0", "sum", "a1"]);
}

#[test]
fn is_context_overflow_error_matches_common_phrases() {
    assert!(is_context_overflow_error(&anyhow::anyhow!(
        "HTTP 400: context_length_exceeded"
    )));
    assert!(is_context_overflow_error(&anyhow::anyhow!(
        "maximum context length is 128000 tokens"
    )));
    assert!(is_context_overflow_error(&anyhow::anyhow!(
        "Range of input is too long"
    )));
    assert!(!is_context_overflow_error(&anyhow::anyhow!(
        "HTTP 429 rate limit"
    )));
    assert!(!is_context_overflow_error(&anyhow::anyhow!(
        "connection reset"
    )));
    assert!(is_context_overflow_error(&anyhow::anyhow!(
        "HTTP 413 Request Entity Too Large"
    )));
    assert!(is_context_overflow_error(&anyhow::anyhow!(
        "prompt exceeds the limit of the context window"
    )));
}

#[test]
fn should_precompress_history_uses_soft_gate_and_ratio() {
    let budget = 100_000;
    let soft = precompress_gate_threshold(budget);
    assert_eq!(soft, 80_000);
    let small = vec![u("hi"), u("there")];
    assert!(!should_precompress_history(&small, None, budget, 1));
}

#[test]
fn cjk_dense_includes_fullwidth_and_punctuation() {
    assert!(is_cjk_dense_rune('中'));
    assert!(is_cjk_dense_rune('。'));
    assert!(is_cjk_dense_rune('Ａ'));
    assert!(!is_cjk_dense_rune('A'));
}

#[test]
fn normalize_context_budget_tokens_floors_small_values() {
    assert_eq!(normalize_context_budget_tokens(120_000), 120_000);
    assert_eq!(normalize_context_budget_tokens(1000), 4096);
}

#[test]
fn compression_gate_uses_api_prompt_without_local_estimate() {
    let msgs = vec![u("short")];
    let (gate, payload, api, source) = compression_gate_tokens(&msgs, Some(150_000));
    assert_eq!(payload, 0);
    assert_eq!(api, Some(150_000));
    assert_eq!(gate, 150_000);
    assert_eq!(source, "api_prompt");
}

#[test]
fn compression_gate_uses_payload_when_no_api_report() {
    let long = "word ".repeat(25_000);
    let msgs = vec![u(&long)];
    let payload = estimate_message_payload_tokens(&msgs);
    let (gate, payload2, api, source) = compression_gate_tokens(&msgs, None);
    assert_eq!(payload, payload2);
    assert_eq!(api, None);
    assert_eq!(gate, payload);
    assert_eq!(source, "payload_est");
}

#[test]
fn payload_tokens_include_image_slots() {
    let mut m = u("screen");
    m.images_base64 = Some(vec!["aaa".into(), "bbb".into()]);
    let t = estimate_message_payload_tokens(&[m]);
    assert!(t >= EST_IMAGE_TOKENS_PER_SLOT * 2);
}

#[test]
fn token_estimate_triggers_against_token_budget_for_dense_ascii() {
    let long = "word ".repeat(25_000); // ~31_250 est tokens
    let msgs = vec![u(&long)];
    let est = estimate_message_payload_tokens(&msgs);
    assert!(est > normalize_context_budget_tokens(30_000));
    assert!(est < 125_000);
}

#[test]
fn cjk_history_counts_higher_than_ascii_char_ratio() {
    let ascii = "a".repeat(6000);
    let cjk = "中".repeat(6000);
    let ascii_t = estimate_text_tokens_heuristic(&ascii);
    let cjk_t = estimate_text_tokens_heuristic(&cjk);
    assert_eq!(ascii_t, 1500);
    assert_eq!(cjk_t, 4000);
    assert!(cjk_t > ascii_t);
}

#[test]
fn summary_system_prompt_includes_token_tail_and_explore_hint() {
    let ui = CompressionUiContext::sub_agent(
        AgentInstanceScope::new("test-run", "conv", "explore"),
        "m",
        "explore",
        "Explore Agent",
        "t",
    );
    let p = build_summary_system_prompt(&ui, false);
    assert!(p.contains("## Goal"));
    assert!(p.contains("## Progress"));
    assert!(p.contains("## State"));
    assert!(p.contains("## Open"));
    assert!(!p.contains("## Active Task"));
    assert!(!p.contains("## Pending User Asks"));
    assert!(p.contains("recent-message tail"));
    assert!(p.contains("read-only explore"));
    assert!(p.contains("[REDACTED]"));
}

#[test]
fn summary_system_prompt_has_forgetting_rules() {
    let ui = CompressionUiContext::main(AgentInstanceScope::new("test-run", "conv", "main"));
    let p = build_summary_system_prompt(&ui, false);
    assert!(p.contains("Forgetting rules"));
    assert!(p.contains("previous conversation-summary"));
    assert!(p.contains("Silence is not a drop"));
    assert!(p.contains("same topic (superseded)"));
    assert!(p.contains("latest version of each topic"));
    assert!(p.contains("env vars"));
    assert!(p.contains("command + pass/fail"));
    let in_run = build_summary_system_prompt(&ui, true);
    assert!(in_run.contains("Silence is not a drop"));
    assert!(in_run.contains("same topic (superseded)"));
    assert!(in_run.contains("## State"));
    assert!(!in_run.contains("## Key findings"));
    assert!(in_run.contains("agreed copy"));
}

#[test]
fn summary_user_prompt_frames_source_and_repeats_instructions_after_it() {
    let prompt = build_summary_user_prompt("[USER]: continue the conversation", 5_400, false);
    let source_end = prompt.find("--- END SOURCE CONVERSATION ---").unwrap();
    let final_instruction = prompt
        .rfind("Do NOT answer, continue, or fulfill any question")
        .unwrap();

    assert!(prompt.contains("--- BEGIN SOURCE CONVERSATION ---"));
    assert!(prompt.contains("[USER]: continue the conversation"));
    assert!(prompt.contains("5400 tokens is a HARD CEILING"));
    assert!(!prompt.contains("truncated output is rejected"));
    assert!(prompt.contains("Goal > Progress (blockers and decisions) > State > Open"));
    let in_run = build_summary_user_prompt("[USER]: continue the conversation", 5_400, true);
    assert!(in_run.contains("Progress (blockers and decisions) > State > Next"));
    assert!(prompt.contains("One line per action"));
    assert!(final_instruction > source_end);
    let language_at = prompt.find("LANGUAGE:").expect("language rule after source");
    assert!(language_at > source_end);
    assert!(prompt.contains("same language the USER turns mainly used"));
    assert!(prompt.ends_with(
            "Write only the summary body. Do not include a greeting, preamble, or response to the conversation."
        ));
}

#[test]
fn compact_retry_prompts_drop_state_and_avoid_filling_the_cap() {
    let ui = CompressionUiContext::main(AgentInstanceScope::new("test-run", "conv", "main"));
    let prefix = build_summary_system_prompt_with_style(&ui, false, true);
    assert!(prefix.contains("## Goal"));
    assert!(prefix.contains("## Progress"));
    assert!(prefix.contains("## Open"));
    assert!(!prefix.contains("## State"));
    assert!(!prefix.contains("TEMPORAL ANCHORING"));
    assert!(prefix.contains("Do not list every tool call."));
    assert!(prefix.contains("Do not copy raw tool dumps."));

    let in_run = build_summary_system_prompt_with_style(&ui, true, true);
    assert!(in_run.contains("## Progress"));
    assert!(in_run.contains("## Next"));
    assert!(!in_run.contains("## State"));
    assert!(!in_run.contains("## Goal"));
    assert!(!in_run.contains("TEMPORAL ANCHORING"));

    let user = build_summary_user_prompt_with_style("[USER]: continue", 10_800, false, true);
    assert!(user.contains("Stay well under 10800 tokens"));
    assert!(user.contains("Do not fill the allowance"));
    assert!(!user.contains("HARD CEILING"));
    assert!(user.contains("Goal > Progress (blockers and latest decisions) > Open"));
    assert!(!user.contains("## State"));
    assert!(!user.contains("BEGIN TRUNCATED DRAFT"));

    let in_run_user = build_summary_user_prompt_with_style("[USER]: continue", 10_800, true, true);
    assert!(in_run_user.contains("Progress (blockers and latest decisions) > Next"));
    assert!(in_run_user.contains("## Next"));
    assert!(!in_run_user.contains("## State"));
    assert!(!in_run_user.contains("BEGIN TRUNCATED DRAFT"));
}

#[test]
fn detect_summary_body_language_follows_real_user_turns() {
    assert_eq!(
        detect_summary_body_language(&[u("请帮我修一下上下文压缩")]),
        SummaryBodyLanguage::Chinese
    );
    assert_eq!(
        detect_summary_body_language(&[u("please fix context compression")]),
        SummaryBodyLanguage::English
    );
    let mut toolish = u("error: file not found at src/lib.rs");
    toolish.role = Role::Tool;
    assert_eq!(
        detect_summary_body_language(&[u("把这个报错修好"), toolish]),
        SummaryBodyLanguage::Chinese
    );
}

#[test]
fn summary_prompts_pin_body_language_after_the_source() {
    let ui = CompressionUiContext::main(AgentInstanceScope::new("test-run", "conv", "main"));
    let system = build_summary_system_prompt_with_language(
        &ui,
        false,
        false,
        SummaryBodyLanguage::Chinese,
    );
    assert!(system.contains("Write every section body in Chinese"));
    assert!(system.contains("write 无"));
    assert!(system.contains("Do not write bodies in English because tool output is English"));

    let user = build_summary_user_prompt_with_language(
        "--- user ---\n请继续",
        5_400,
        false,
        false,
        SummaryBodyLanguage::Chinese,
    );
    let source_end = user.find("--- END SOURCE CONVERSATION ---").unwrap();
    let language_at = user.find("LANGUAGE:").unwrap();
    assert!(language_at > source_end);
    assert!(user.contains("Write every section body in Chinese"));
}

#[test]
fn length_output_fallback_keeps_raw_truncated_text() {
    assert!(length_output_for_fallback("   \n").is_none());
    let kept = length_output_for_fallback("## Progress\nkept\n## State\nfiles").unwrap();
    assert_eq!(kept, "## Progress\nkept\n## State\nfiles");
    let fallback = keep_length_output_fallback("conv", Some(kept.clone()));
    assert_eq!(fallback.as_deref(), Some(kept.as_str()));
    assert!(keep_length_output_fallback("conv", None).is_none());
}

#[test]
fn summary_max_tokens_requested_uses_1_5x_headroom() {
    assert_eq!(summary_max_tokens_requested(5_400), 8_100);
    assert_eq!(summary_max_tokens_requested(1_500), 2_250);
    assert_eq!(summary_max_tokens_requested(12_000), 18_000);
}

#[test]
fn summary_max_tokens_retry_uses_3x_budget() {
    assert_eq!(summary_max_tokens_retry(5_400), 16_200);
    assert_eq!(summary_max_tokens_retry(1_500), 4_500);
}

#[test]
fn summary_max_tokens_retry_prompt_uses_2x_budget() {
    assert_eq!(summary_max_tokens_retry_prompt(5_400), 10_800);
    assert_eq!(summary_max_tokens_retry_prompt(1_500), 3_000);
}

#[test]
fn should_retry_summary_on_reject_only_for_length() {
    assert!(should_retry_summary_on_reject("finish_reason=length"));
    assert!(should_retry_summary_on_reject("finish_reason=LENGTH"));
    assert!(!should_retry_summary_on_reject("empty output"));
    assert!(!should_retry_summary_on_reject(
        "finish_reason=content_filter"
    ));
    assert!(!should_retry_summary_on_reject("finish_reason=stop"));
}

#[test]
fn persisted_summary_marks_compacted_content_as_reference_only() {
    let body = build_persisted_summary(SUMMARY_PREFIX_BUDGET, "## Decisions\n- Keep it.");
    assert!(body.starts_with(SUMMARY_PREFIX_BUDGET));
    assert!(body.contains("[REFERENCE ONLY]"));
    assert!(body.contains("not as a new user request"));
    assert!(body.ends_with("## Decisions\n- Keep it."));
}

#[test]
fn summary_input_preserves_anchor_and_split_adjacent_tail() {
    let mut messages = Vec::new();
    for index in 0..35 {
        let marker = if index == 0 {
            "ORIGINAL_GOAL"
        } else if index == 34 {
            "SPLIT_ADJACENT_TASK_STATE"
        } else {
            "middle"
        };
        messages.push(u(&format!("{marker}-{}", "x".repeat(5_000))));
    }

    let formatted = format_prefix_for_summary(&messages);
    assert!(formatted.contains("ORIGINAL_GOAL"));
    assert!(formatted.contains("SPLIT_ADJACENT_TASK_STATE"));
    assert!(formatted.contains("omitted"));
    assert!(formatted.chars().count() <= MAX_PREFIX_CHARS_FOR_API);
}

#[test]
fn summary_input_ignores_soft_excluded_rows_after_reload() {
    let mut excluded = u("STALE_EXCLUDED_CONTEXT");
    crate::message_context::mark_excluded(
        &mut excluded,
        crate::models::ExcludedReason::ContextCompression,
    );
    let formatted = format_prefix_for_summary(&[excluded, u("ACTIVE_CONTEXT")]);
    assert!(!formatted.contains("STALE_EXCLUDED_CONTEXT"));
    assert!(formatted.contains("ACTIVE_CONTEXT"));
}

fn summary_output(text: String, finish_reason: Option<&str>) -> crate::provider::ChatOnceOutput {
    crate::provider::ChatOnceOutput {
        text,
        usage: None,
        model: "test-model".into(),
        tool_calls: vec![],
        reasoning_content: None,
        finish_reason: finish_reason.map(str::to_string),
    }
}

fn prefix_full_headings() -> String {
    "## Goal\nFix compression.\n## Progress\nRetried once.\n## State\nOn main.\n## Open\nNone."
        .into()
}

fn in_run_full_headings() -> String {
    "## Progress\nPatched validate.\n## State\nsummary.rs.\n## Next\nRetry compact.".into()
}

fn compact_in_run_headings() -> String {
    "## Progress\nDropped tool dump.\n## Next\nContinue the turn.".into()
}

#[test]
fn summary_validation_accepts_nonempty_unstructured_output() {
    let summary = "用户目标：修复压缩失败。\n当前状态：继续处理。";
    assert!(
        validate_summary_output(&summary_output(summary.into(), Some("stop")), false, false)
            .is_ok()
    );
}

#[test]
fn summary_validation_rejects_empty_output() {
    let error = validate_summary_output(&summary_output("  \n".into(), Some("stop")), false, false)
        .expect_err("empty output must not be accepted");
    assert_eq!(error, "empty output");
}

#[test]
fn summary_validation_rejects_length_without_required_headings() {
    let error = validate_summary_output(
        &summary_output("partial summary".into(), Some("length")),
        false,
        false,
    )
    .expect_err("length output without headings must not be accepted");
    assert!(error.contains("finish_reason=length"));
}

#[test]
fn summary_validation_accepts_length_with_required_headings() {
    assert!(validate_summary_output(
        &summary_output(prefix_full_headings(), Some("length")),
        false,
        false,
    )
    .is_ok());
    assert!(validate_summary_output(
        &summary_output(in_run_full_headings(), Some("length")),
        true,
        false,
    )
    .is_ok());
    assert!(validate_summary_output(
        &summary_output(compact_in_run_headings(), Some("length")),
        true,
        true,
    )
    .is_ok());
}

#[test]
fn summary_validation_rejects_length_when_a_heading_is_missing() {
    let missing_state = "## Progress\nDid work.\n## Next\nContinue.";
    let error = validate_summary_output(
        &summary_output(missing_state.into(), Some("length")),
        true,
        false,
    )
    .expect_err("full in-run template still requires State");
    assert!(error.contains("finish_reason=length"));
}

#[test]
fn remember_session_llm_roundtrip() {
    let mut settings = crate::models::ModelSettings::default();
    settings.active_provider_id = "deepseek".into();
    settings.model = "deepseek-v4-flash".into();
    remember_session_llm("test-session-llm-roundtrip", &settings, "k");
    let snap = session_llm_for_conversation("test-session-llm-roundtrip").expect("remembered");
    assert_eq!(snap.settings.active_provider_id, "deepseek");
    assert_eq!(snap.settings.model, "deepseek-v4-flash");
    assert_eq!(snap.api_key, "k");
}

#[test]
fn pending_apply_invalidates_stale_api_tokens_and_gate_skips() {
    let mut summary = new_summary_message("already compressed".into(), false);
    summary.id = "sum_applied".into();
    let mut current = u("current question");
    current.id = "cur_applied".into();
    let history = vec![summary, current];
    let budget = 10_000;
    let stale = plan_compression(&history, Some(200_000), budget, 1, false, false, false);
    assert!(
        !matches!(stale, CompressionPlan::Skip),
        "stale api tokens must still trip the gate: {stale:?}"
    );
    let tokens = prompt_tokens_after_pending_apply(true, Some(200_000));
    assert_eq!(tokens, None);
    assert!(matches!(
        plan_compression(&history, tokens, budget, 1, false, false, false),
        CompressionPlan::Skip
    ));
    assert_eq!(
        prompt_tokens_after_pending_apply(false, Some(200_000)),
        Some(200_000)
    );
}

#[test]
fn second_compress_allowed_when_local_estimate_still_over() {
    let mut history = Vec::new();
    for i in 0..8 {
        let mut m = u(&"old turn body ".repeat(4_000));
        m.id = format!("old_{i}");
        history.push(m);
    }
    let mut current = u("keep this question");
    current.id = "keep_q".into();
    history.push(current);
    let plan = plan_compression(&history, None, 2_000, 1, false, false, false);
    assert!(
        !matches!(plan, CompressionPlan::Skip),
        "real leftover prefix must still compress: {plan:?}"
    );
    match plan {
        CompressionPlan::Prefix { split } => {
            assert!(super::run::droppable_message_count(&history, 0, split) > 0);
        }
        CompressionPlan::InRun {
            drop_start,
            tail_start,
        } => {
            assert!(super::run::droppable_message_count(&history, drop_start, tail_start) > 0);
        }
        CompressionPlan::Skip => unreachable!(),
    }
}

#[tokio::test]
async fn empty_drop_window_does_not_emit_started_or_insert_summary() {
    let mut settings = crate::models::ModelSettings::default();
    settings.context_compression_enabled = true;
    settings.context_budget_tokens = 1_000;
    settings.context_keep_recent_user_turns = 1;
    let provider = crate::provider::OpenAIProvider::new(settings.clone(), "sk-test".into());
    let (stream, mut rx) = crate::models::ChatStreamSender::pair("c_empty_drop", "");
    let mut summary = u(&format!("{SUMMARY_PREFIX_BUDGET}\nalready compressed"));
    summary.id = "only_sum".into();
    let mut current = u("current question");
    current.id = "only_cur".into();
    let mut history = vec![summary, current];
    let changed = super::run::compress_history_inner(
        &mut history,
        &settings,
        &provider,
        "c_empty_drop",
        &stream,
        tokio_util::sync::CancellationToken::new(),
        true,
        false,
        false,
        true,
        &CompressionUiContext::default(),
        Some(200_000),
        None,
        false,
    )
    .await;
    assert!(!changed);
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].id, "only_sum");
    assert_eq!(history[1].id, "only_cur");
    drop(stream);
    let mut saw_started = false;
    while let Ok(ev) = rx.try_recv() {
        if matches!(
            ev,
            crate::models::StreamEvent::ContextCompressionStarted { .. }
        ) {
            saw_started = true;
        }
    }
    assert!(!saw_started);
}
