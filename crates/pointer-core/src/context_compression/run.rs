//! Apply a compression plan to live history (sync paths).

use super::assembly::*;
use super::budget::*;
use super::precompress::{enqueue_pending_splice, PendingCompressionSplice};
use super::summary::*;
use super::types::*;
use crate::models::{ChatMessage, ModelSettings, Role, StreamEvent};
use crate::provider::OpenAIProvider;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

pub(crate) fn droppable_message_count(
    history: &[ChatMessage],
    drop_start: usize,
    drop_end: usize,
) -> u32 {
    if drop_start >= drop_end || drop_end > history.len() {
        return 0;
    }
    history[drop_start..drop_end]
        .iter()
        .filter(|m| {
            crate::message_context::is_context_included(m)
                && !crate::message_context::is_synthetic_user_content(&m.content)
        })
        .count() as u32
}

/// Last N real user turns (user + concluding assistant) that fall inside the
/// drop window. Used when the summary LLM fails so those turns stay verbatim.
pub(crate) fn drop_fallback_keep_ids(
    history: &[ChatMessage],
    drop_start: usize,
    drop_end: usize,
) -> HashSet<String> {
    let mut keep = HashSet::new();
    if drop_start >= drop_end || drop_end > history.len() {
        return keep;
    }
    for user_idx in crate::message_context::find_recent_context_user_indices(
        history,
        DROP_FALLBACK_KEEP_USER_TURNS,
    ) {
        if user_idx >= drop_start && user_idx < drop_end {
            keep.insert(history[user_idx].id.clone());
        }
        if let Some(asst_idx) =
            crate::message_context::concluding_assistant_index(history, user_idx)
        {
            if asst_idx >= drop_start && asst_idx < drop_end {
                keep.insert(history[asst_idx].id.clone());
            }
        }
    }
    keep
}

pub(crate) fn fingerprint_window_start(history: &[ChatMessage], ids: &[String]) -> Option<usize> {
    let n = ids.len();
    if n == 0 || history.len() < n {
        return None;
    }
    (0..=history.len() - n).find(|&start| {
        history[start..start + n]
            .iter()
            .zip(ids.iter())
            .all(|(m, id)| m.id == *id)
    })
}

/// Remove `drop_ids` from `[drop_start, drop_end)`, then insert `summary` at
/// `drop_start` so salvaged rows stay after the handoff and before the tail.
pub(crate) fn splice_summary_into_drop_window(
    history: &mut Vec<ChatMessage>,
    drop_start: usize,
    drop_end: usize,
    drop_ids: &HashSet<String>,
    summary: ChatMessage,
) {
    if drop_start > drop_end || drop_end > history.len() {
        log::warn!(
            "context_compress: splice window invalid drop_start={drop_start} drop_end={drop_end} len={}",
            history.len()
        );
        return;
    }
    for i in (drop_start..drop_end).rev() {
        if drop_ids.contains(&history[i].id) {
            history.remove(i);
        }
    }
    let insert_at = drop_start.min(history.len());
    history.insert(insert_at, summary);
}

struct DropSplicePlan {
    keep_ids: HashSet<String>,
    excluded_for_persist: Vec<ChatMessage>,
    insert_before_message_id: String,
    dropped_count: u32,
}

fn plan_drop_splice(
    history: &[ChatMessage],
    drop_start: usize,
    drop_end: usize,
    summary_failed: bool,
) -> DropSplicePlan {
    let keep_ids = if summary_failed {
        drop_fallback_keep_ids(history, drop_start, drop_end)
    } else {
        HashSet::new()
    };
    let mut excluded_for_persist = Vec::new();
    for m in &history[drop_start..drop_end] {
        if keep_ids.contains(&m.id) {
            continue;
        }
        if crate::message_context::is_context_included(m) {
            let mut excluded = m.clone();
            crate::message_context::mark_excluded(
                &mut excluded,
                crate::models::ExcludedReason::ContextCompression,
            );
            excluded_for_persist.push(excluded);
        }
    }
    let dropped_count = excluded_for_persist
        .iter()
        .filter(|m| !crate::message_context::is_synthetic_user_content(&m.content))
        .count() as u32;
    let insert_before_message_id = history[drop_start..drop_end]
        .iter()
        .find(|m| keep_ids.contains(&m.id))
        .or_else(|| history.get(drop_end))
        .map(|m| m.id.clone())
        .unwrap_or_default();
    DropSplicePlan {
        keep_ids,
        excluded_for_persist,
        insert_before_message_id,
        dropped_count,
    }
}

/// Decide whether this compression call may reuse the session round's assembly.
///
/// Reuse means sending `[session system][session tools][history[..drop_end]]`
/// plus a trailing instruction, so the provider prefix cache serves everything
/// except that instruction (Codex runs compaction on the live prompt the same
/// way). Overflow recovery is excluded: there the session prompt itself was
/// rejected for length, so it cannot be re-sent as-is.
pub(crate) fn resolve_assembly_reuse(
    history: &[ChatMessage],
    settings: &ModelSettings,
    conversation_id: &str,
    ui: &CompressionUiContext,
    overflow_recover: bool,
) -> Option<SessionPromptSnapshot> {
    if overflow_recover {
        log::debug!(
            "context_compress: assembly reuse skipped (overflow recovery) conversation_id={conversation_id}"
        );
        return None;
    }
    let agent_instance_id = scope_key_from_ui(ui);
    let Some(snapshot) = session_prompt_for(conversation_id, agent_instance_id) else {
        log::debug!(
            "context_compress: assembly reuse skipped (no session snapshot) conversation_id={conversation_id} agent_instance_id={}",
            agent_instance_id.unwrap_or("-")
        );
        return None;
    };
    if !snapshot.matches_settings(settings) {
        log::info!(
            "context_compress: assembly reuse skipped (provider/model changed) conversation_id={conversation_id} snapshot={}/{} current={}/{}",
            snapshot.provider_id,
            snapshot.model,
            settings.active_provider_id,
            settings.model
        );
        return None;
    }
    if !snapshot.covers_history(history) {
        log::warn!(
            "context_compress: assembly reuse skipped (stale snapshot) conversation_id={conversation_id} last_message_id={}",
            snapshot.last_message_id
        );
        return None;
    }
    log::info!(
        "context_compress: assembly reuse conversation_id={conversation_id} scope={:?} tools={} cacheable_slices={} dynamic_slices={}",
        snapshot.history_scope,
        snapshot.native_tools.len(),
        snapshot.system.cacheable.len(),
        snapshot.system.dynamic.len()
    );
    Some(snapshot)
}

/// Prompt-side tokens the session assembly adds on top of the summarized
/// history (system slices + tool schemas), so the fitted window accounts for
/// them instead of assuming the whole budget belongs to history.
fn assembly_overhead_tokens(snapshot: &SessionPromptSnapshot) -> usize {
    let system: usize = snapshot
        .system
        .cacheable
        .iter()
        .chain(snapshot.system.dynamic.iter())
        .map(|s| estimate_text_tokens_heuristic(s))
        .sum();
    let tools = serde_json::to_string(&snapshot.native_tools)
        .map(|json| estimate_text_tokens_heuristic(&json))
        .unwrap_or(0);
    system + tools
}

/// Largest `end` in `(drop_start, drop_end]` whose shared prefix still fits the
/// prompt budget once the checkpoint output is reserved.
///
/// The walk goes backwards from the end of the summarized window, never from its
/// start: everything we do send stays byte-identical to the session request, so
/// the prefix cache still serves it, and the messages that fall out of the
/// window stay verbatim in history instead of being dropped unsummarized.
/// `None` when even the smallest window cannot fit — the legacy compact prompt
/// then takes over.
pub(crate) fn fit_assembly_drop_end(
    history: &[ChatMessage],
    drop_start: usize,
    drop_end: usize,
    settings: &ModelSettings,
    overhead_tokens: usize,
) -> Option<usize> {
    let mut end = drop_end.min(history.len());
    let mut tokens = estimate_message_payload_tokens(&history[..end]);
    while end > drop_start {
        // Provider-side cap of the summary call: the request declares `×1.5`
        // over the prompt target, and that is what has to fit.
        let summary_out =
            summary_max_tokens_requested(compute_summary_max_tokens(tokens)) as usize;
        if tokens + overhead_tokens <= assembly_window_limit(settings, summary_out) {
            return Some(end);
        }
        end -= 1;
        tokens = tokens.saturating_sub(estimate_one_message_payload_tokens(&history[end]));
    }
    None
}

/// Prompt-side limit for the compaction request itself: the model window minus
/// this call's own output reservation.
///
/// Deliberately not [`compress_budget_tokens`]: that one reserves **this
/// round's** `max_tokens` for the session answer, so reusing it here subtracted
/// the session reserve a second time (64k of a 256k window) and trimmed the
/// shared prefix long before the provider would have objected.
pub(crate) fn assembly_window_limit(settings: &ModelSettings, summary_out: usize) -> usize {
    let configured =
        normalize_context_budget_tokens(crate::models::effective_context_budget_tokens(settings));
    apply_completion_reserve(configured, remembered_model_context_window(settings), summary_out)
}

pub(crate) async fn compress_history_inner(
    history: &mut Vec<ChatMessage>,
    settings: &ModelSettings,
    provider: &OpenAIProvider,
    conversation_id: &str,
    stream: &StreamTx,
    cancel: CancellationToken,
    force_ignore_char_budget: bool,
    overflow_recover: bool,
    emit_compression_ui: bool,
    ui: &CompressionUiContext,
    reported_prompt_tokens: Option<u32>,
    // When set (precompress), skip SQLite splice if this conversation has an
    // active run_chat and enqueue the result for idle apply instead.
    defer_persist_state: Option<Arc<crate::chat_service::AppState>>,
    // Sub-agent background job: always enqueue for the next isolated round.
    force_enqueue: bool,
) -> bool {
    let wall = Instant::now();
    let messages_before = history.len();
    let keep_users = settings.context_keep_recent_user_turns.max(1);
    let budget_tokens = compress_budget_tokens(settings);

    let gate = evaluate_compress_gate(
        history,
        reported_prompt_tokens,
        budget_tokens,
        keep_users as usize,
    );
    let (gate_tokens, payload_est, api_prompt, gate_source) = (
        gate.total,
        gate.payload_est,
        gate.api_prompt,
        gate.gate_source,
    );
    let plan = plan_compression(
        history,
        reported_prompt_tokens,
        budget_tokens,
        keep_users as usize,
        overflow_recover,
        force_ignore_char_budget,
    );
    let (drop_start, mut drop_end, in_run) = match plan {
        CompressionPlan::Skip => {
            log::debug!(
                "context_compress: skip_gate conversation_id={} total={} prefix={} ratio={:.3} threshold={} split={} share={:.3} budget_tokens={} wall_ms={}",
                conversation_id,
                gate.total,
                gate.prefix,
                gate.ratio,
                gate.threshold,
                gate.split,
                current_turn_token_share(history),
                budget_tokens,
                wall.elapsed().as_millis()
            );
            return false;
        }
        CompressionPlan::Prefix { split } => (0usize, split, false),
        CompressionPlan::InRun {
            drop_start,
            tail_start,
        } => (drop_start, tail_start, true),
    };
    if drop_end == 0 || drop_start >= drop_end || drop_end > history.len() {
        log::info!(
            "context_compress: skip_no_summary_split conversation_id={} messages={} in_run={} drop_start={} drop_end={} gate_tokens={} gate_source={} payload_est={} api_prompt={:?} wall_ms={}",
            conversation_id,
            messages_before,
            in_run,
            drop_start,
            drop_end,
            gate_tokens,
            gate_source,
            payload_est,
            api_prompt,
            wall.elapsed().as_millis()
        );
        return false;
    }

    // Compression reuses the session round's own assembly when it can (Codex
    // runs compaction on the live prompt). If the shared prefix does not fit,
    // walk the summarized window's end back toward its start until it does — no
    // separate size gate: trimming the end keeps everything we send
    // byte-identical to the session request, and messages that fall out of the
    // window stay verbatim instead of being dropped unsummarized.
    let mut assembly =
        resolve_assembly_reuse(history, settings, conversation_id, ui, overflow_recover);
    if assembly.is_some() {
        let overhead_tokens = assembly
            .as_ref()
            .map(assembly_overhead_tokens)
            .unwrap_or(0);
        match fit_assembly_drop_end(history, drop_start, drop_end, settings, overhead_tokens) {
            Some(fitted) => {
                if fitted != drop_end {
                    log::info!(
                        "context_compress: assembly reuse window trimmed conversation_id={conversation_id} drop_end={drop_end} fitted={fitted} overhead_tokens={overhead_tokens} budget={}",
                        compress_budget_tokens(settings)
                    );
                    drop_end = fitted;
                }
            }
            None => {
                log::info!(
                    "context_compress: assembly reuse skipped (prefix does not fit after trimming) conversation_id={conversation_id} overhead_tokens={overhead_tokens} window_limit={}",
                    assembly_window_limit(
                        settings,
                        summary_max_tokens_requested(SUMMARY_TOKENS_CEILING) as usize
                    )
                );
                assembly = None;
            }
        }
    }

    let summary_source = &history[..drop_end];
    if summary_source.is_empty() {
        log::info!(
            "context_compress: skip_empty_prefix conversation_id={} messages={} wall_ms={}",
            conversation_id,
            messages_before,
            wall.elapsed().as_millis()
        );
        return false;
    }

    let dropped_count = droppable_message_count(history, drop_start, drop_end);
    if dropped_count == 0 {
        log::info!(
            "context_compress: skip_empty_drop_window conversation_id={} messages={} in_run={} drop_start={} drop_end={} gate_tokens={} gate_source={} payload_est={} api_prompt={:?} wall_ms={}",
            conversation_id,
            messages_before,
            in_run,
            drop_start,
            drop_end,
            gate_tokens,
            gate_source,
            payload_est,
            api_prompt,
            wall.elapsed().as_millis()
        );
        return false;
    }

    // In-thread marker at the summary split (frontend); completion still uses UiToast.
    if emit_compression_ui {
        let insert_before = history
            .get(drop_end)
            .map(|m| m.id.clone())
            .filter(|id| !id.trim().is_empty());
        emit_compression_started(stream, conversation_id, ui, insert_before);
    }

    let t_fmt = Instant::now();
    let formatted = if assembly.is_none() {
        format_prefix_for_summary(summary_source)
    } else {
        String::new()
    };
    let format_prefix_ms = t_fmt.elapsed().as_millis();
    let content_tokens = if assembly.is_some() {
        estimate_message_payload_tokens(summary_source)
    } else {
        estimate_text_tokens_heuristic(&formatted)
    };
    let max_tok = compute_summary_max_tokens(content_tokens);
    // Provider cap gets 1.5× headroom over the target budget so the model can
    // close out without `finish_reason=length`. The budget itself stays the
    // prompt target (~N tokens) and the log label.
    let requested_max_tok = summary_max_tokens_requested(max_tok);
    let body_language = detect_summary_body_language(summary_source);
    log::info!(
        "context_compress: summary_language conversation_id={} lang={:?}",
        conversation_id,
        body_language
    );
    let summary_user_prompt = match assembly.as_ref() {
        Some(_) => build_compaction_instruction_message_with_language(
            ui,
            in_run,
            false,
            body_language,
            max_tok,
        ),
        None => build_summary_user_prompt_with_language(
            &formatted,
            max_tok,
            in_run,
            false,
            body_language,
        ),
    };
    let input = ChatMessage {
        id: format!("sum_in_{}", uuid::Uuid::new_v4().simple()),
        role: Role::User,
        content: summary_user_prompt,
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
    };

    let summary_prefix = if force_ignore_char_budget {
        SUMMARY_PREFIX_TOOL_LIMIT
    } else {
        SUMMARY_PREFIX_BUDGET
    };
    let reason = if in_run {
        if overflow_recover {
            "overflow_in_run"
        } else if force_ignore_char_budget {
            "tool_limit_in_run"
        } else {
            "in_run"
        }
    } else if overflow_recover {
        "overflow"
    } else if force_ignore_char_budget {
        "tool_limit"
    } else {
        "budget"
    };
    let dump_lbl = format!("{}_context_summary_{}", conversation_id, reason);
    log::info!(
        "context_compress: summary_budget conversation_id={} content_tokens={} floor={} max_tokens={} requested={} ceiling={}",
        conversation_id,
        content_tokens,
        MIN_SUMMARY_TOKENS,
        max_tok,
        requested_max_tok,
        SUMMARY_TOKENS_CEILING
    );
    let t_llm = Instant::now();
    // Usage of the attempt that actually produced the summary, so the applied
    // line can report prompt / cache-hit tokens for before-after comparisons.
    let mut summary_prompt_tokens: u32 = 0;
    let mut summary_cached_tokens: u32 = 0;
    let summary_system =
        build_summary_system_prompt_with_language(ui, in_run, false, body_language);
    let summary_sections =
        crate::models::SystemPromptSections::all_cacheable(vec![summary_system.clone()]);
    // First attempt: full template, prompt target N, API cap 1.5N.
    // Truncated output with required headings is accepted. Otherwise a
    // compact retry rebuilds the prompt from scratch (target 2N, API cap 3N).
    // If that retry is also truncated without headings, keep the retry text.
    let first_attempt = match assembly.clone().as_ref() {
        Some(snapshot) => {
            let reused = provider
                .chat_once_with_assembly(
                    summary_source,
                    std::slice::from_ref(&input),
                    &snapshot.system,
                    snapshot.native_tools.clone(),
                    snapshot.history_scope,
                    cancel.clone(),
                    Some(requested_max_tok),
                    Some(dump_lbl.as_str()),
                )
                .await;
            match reused {
                Ok(out) => Ok(out),
                Err(e) => {
                    // A provider that rejects the assembly shape (for example a
                    // gateway without `tool_choice: "none"`) must not lose
                    // compression entirely: retry once with the curated
                    // summarizer prompt, and keep the legacy path for the retry.
                    log::warn!(
                        "context_compress: assembly reuse call failed, falling back to the legacy summary prompt conversation_id={conversation_id} error={e:#}"
                    );
                    assembly = None;
                    let mut legacy_input = input.clone();
                    legacy_input.content = build_summary_user_prompt_with_language(
                        &format_prefix_for_summary(summary_source),
                        max_tok,
                        in_run,
                        false,
                        body_language,
                    );
                    provider
                        .chat_once_without_thinking(
                            std::slice::from_ref(&legacy_input),
                            &summary_sections,
                            Vec::new(),
                            cancel.clone(),
                            Some(requested_max_tok),
                            Some(dump_lbl.as_str()),
                        )
                        .await
                }
            }
        }
        None => {
            provider
                .chat_once_without_thinking(
                    std::slice::from_ref(&input),
                    &summary_sections,
                    Vec::new(),
                    cancel.clone(),
                    Some(requested_max_tok),
                    Some(dump_lbl.as_str()),
                )
                .await
        }
    };
    let summary_text = match first_attempt
    {
        Ok(out) => {
            record_summary_usage(
                ui,
                &out,
                "no_thinking",
                crate::llm_token_stats::active_provider_source(&provider.settings),
            );
            match validate_summary_output(&out, in_run, false) {
                Ok(text) => {
                    if let Some(u) = out.usage.as_ref() {
                        summary_prompt_tokens = u.prompt_tokens;
                        summary_cached_tokens = u.cached_tokens;
                    }
                    log_truncated_summary_accept(
                        conversation_id,
                        "no_thinking",
                        &out,
                        in_run,
                        false,
                    );
                    Some(text)
                }
                Err(reason) => {
                    let model = crate::llm_token_stats::model_name_for_usage_report(&out.model);
                    log::warn!(
                        "context summary rejected conversation_id={} attempt=no_thinking reason={} model={model:?} finish_reason={:?} completion_tokens={} prompt_tokens={} max_tokens={} requested={} summary_llm_ms={}",
                        conversation_id,
                        reason,
                        out.finish_reason,
                        out.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0),
                        out.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0),
                        max_tok,
                        requested_max_tok,
                        t_llm.elapsed().as_millis(),
                    );
                    if should_retry_summary_on_reject(&reason) {
                        let retry_prompt_target = summary_max_tokens_retry_prompt(max_tok);
                        let retry_max = summary_max_tokens_retry(max_tok);
                        let retry_sections = if assembly.is_some() {
                            crate::models::SystemPromptSections::default()
                        } else {
                            let retry_system = build_summary_system_prompt_with_language(
                                ui,
                                in_run,
                                true,
                                body_language,
                            );
                            crate::models::SystemPromptSections::all_cacheable(vec![retry_system])
                        };
                        let mut retry_input = input.clone();
                        retry_input.content = match assembly.as_ref() {
                            Some(_) => build_compaction_instruction_message_with_language(
                                ui,
                                in_run,
                                true,
                                body_language,
                                retry_prompt_target,
                            ),
                            None => build_summary_user_prompt_with_language(
                                &formatted,
                                retry_prompt_target,
                                in_run,
                                true,
                                body_language,
                            ),
                        };
                        log::info!(
                            "context summary retrying conversation_id={} attempt=retry_length compact=true budget={} prompt_target={} requested={} summary_llm_ms={}",
                            conversation_id,
                            max_tok,
                            retry_prompt_target,
                            retry_max,
                            t_llm.elapsed().as_millis(),
                        );
                        let retry_attempt = match assembly.as_ref() {
                            Some(snapshot) => {
                                provider
                                    .chat_once_with_assembly(
                                        summary_source,
                                        std::slice::from_ref(&retry_input),
                                        &snapshot.system,
                                        snapshot.native_tools.clone(),
                                        snapshot.history_scope,
                                        cancel.clone(),
                                        Some(retry_max),
                                        Some(dump_lbl.as_str()),
                                    )
                                    .await
                            }
                            None => {
                                provider
                                    .chat_once_without_thinking(
                                        std::slice::from_ref(&retry_input),
                                        &retry_sections,
                                        Vec::new(),
                                        cancel.clone(),
                                        Some(retry_max),
                                        Some(dump_lbl.as_str()),
                                    )
                                    .await
                            }
                        };
                        match retry_attempt
                        {
                            Ok(out2) => {
                                record_summary_usage(
                                    ui,
                                    &out2,
                                    "retry_length",
                                    crate::llm_token_stats::active_provider_source(
                                        &provider.settings,
                                    ),
                                );
                                match validate_summary_output(&out2, in_run, true) {
                                    Ok(text) => {
                                        if let Some(u) = out2.usage.as_ref() {
                                            summary_prompt_tokens = u.prompt_tokens;
                                            summary_cached_tokens = u.cached_tokens;
                                        }
                                        log_truncated_summary_accept(
                                            conversation_id,
                                            "retry_length",
                                            &out2,
                                            in_run,
                                            true,
                                        );
                                        Some(text)
                                    }
                                    Err(reason2) => {
                                        let model2 =
                                            crate::llm_token_stats::model_name_for_usage_report(
                                                &out2.model,
                                            );
                                        log::warn!(
                                            "context summary rejected conversation_id={} attempt=retry_length reason={} model={model2:?} finish_reason={:?} completion_tokens={} prompt_tokens={} max_tokens={} requested={} summary_llm_ms={}",
                                            conversation_id,
                                            reason2,
                                            out2.finish_reason,
                                            out2.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0),
                                            out2.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0),
                                            max_tok,
                                            retry_max,
                                            t_llm.elapsed().as_millis(),
                                        );
                                        if should_retry_summary_on_reject(&reason2) {
                                            keep_length_output_fallback(
                                                conversation_id,
                                                length_output_for_fallback(&out2.text),
                                            )
                                        } else {
                                            None
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                log::warn!(
                                    "context summary LLM call failed conversation_id={} attempt=retry_length error={e:#} budget={} prompt_target={} requested={} summary_llm_ms={}",
                                    conversation_id,
                                    max_tok,
                                    retry_prompt_target,
                                    retry_max,
                                    t_llm.elapsed().as_millis()
                                );
                                None
                            }
                        }
                    } else {
                        None
                    }
                }
            }
        }
        Err(e) => {
            log::warn!(
                "context summary LLM call failed conversation_id={} attempt=no_thinking error={e:#} max_tokens={} requested={} summary_llm_ms={}",
                conversation_id,
                max_tok,
                requested_max_tok,
                t_llm.elapsed().as_millis()
            );
            None
        }
    };

    if cancel.is_cancelled() {
        log::info!(
            "context_compress: cancelled after LLM summary, aborting splice conversation_id={} wall_ms={}",
            conversation_id,
            wall.elapsed().as_millis()
        );
        return false;
    }

    let summary_failed = summary_text.is_none();
    let splice = plan_drop_splice(history, drop_start, drop_end, summary_failed);
    let dropped_count = splice.dropped_count;
    let insert_before_message_id = splice.insert_before_message_id.clone();
    let fingerprint_prefix_ids: Vec<String> = history[drop_start..drop_end]
        .iter()
        .map(|m| m.id.clone())
        .collect();
    let drop_ids: HashSet<String> = splice
        .excluded_for_persist
        .iter()
        .map(|m| m.id.clone())
        .collect();

    let summary_body = match summary_text {
        Some(text) => build_persisted_summary(summary_prefix, &text),
        None => {
            let kept_turns = crate::message_context::find_recent_context_user_indices(
                history,
                DROP_FALLBACK_KEEP_USER_TURNS,
            )
            .into_iter()
            .filter(|&i| i >= drop_start && i < drop_end)
            .count();
            log::warn!(
                "context_compress: drop_without_summary conversation_id={} scope={:?} messages={} in_run={} drop_start={} drop_end={} dropped={} kept_messages={} kept_user_turns={} wall_ms={}",
                conversation_id,
                ui.scope,
                messages_before,
                in_run,
                drop_start,
                drop_end,
                dropped_count,
                splice.keep_ids.len(),
                kept_turns,
                wall.elapsed().as_millis()
            );
            build_drop_without_summary_body(summary_prefix, dropped_count, kept_turns)
        }
    };
    let apply_reason = if summary_failed {
        if in_run {
            "in_run_drop"
        } else if overflow_recover {
            "overflow_drop"
        } else if force_ignore_char_budget {
            "tool_limit_drop"
        } else {
            "budget_drop"
        }
    } else {
        reason
    };

    let turn_busy = force_enqueue
        || (matches!(ui.scope, CompressionScope::Main)
            && defer_persist_state
                .as_ref()
                .map(|s| s.cancels.lock().contains_key(conversation_id))
                .unwrap_or(false));
    if turn_busy {
        let summary_msg = new_summary_message(summary_body, in_run);
        let mut preview_hist = history[..drop_start].to_vec();
        preview_hist.push(summary_msg.clone());
        preview_hist.extend(
            history[drop_start..drop_end]
                .iter()
                .filter(|m| splice.keep_ids.contains(&m.id))
                .cloned(),
        );
        preview_hist.extend(history[drop_end..].iter().cloned());
        let preview_for_disk = crate::conversation_store::conversation_preview(&preview_hist);
        enqueue_pending_splice(PendingCompressionSplice {
            queue_key: compression_queue_key(conversation_id, ui),
            conversation_id: conversation_id.to_string(),
            ui: ui.clone(),
            fingerprint_prefix_ids,
            insert_before_message_id,
            excluded_for_persist: splice.excluded_for_persist,
            summary_msg,
            preview_for_disk,
            dropped_count,
            keep_users,
            apply_reason: apply_reason.to_string(),
            summary_failed,
        });
        log::info!(
            "context_compress: enqueued splice (turn busy) conversation_id={} in_run={} drop_start={} drop_end={} dropped={} kept_messages={} share={:.3} summary_prompt_tokens={} summary_cached_tokens={} wall_ms={}",
            conversation_id,
            in_run,
            drop_start,
            drop_end,
            dropped_count,
            splice.keep_ids.len(),
            current_turn_token_share(history),
            summary_prompt_tokens,
            summary_cached_tokens,
            wall.elapsed().as_millis()
        );
        return false;
    }

    let excluded_message_ids: Vec<String> = splice
        .excluded_for_persist
        .iter()
        .map(|m| m.id.clone())
        .collect();
    for m in &mut history[drop_start..drop_end] {
        if drop_ids.contains(&m.id) && crate::message_context::is_context_included(m) {
            crate::message_context::mark_excluded(
                m,
                crate::models::ExcludedReason::ContextCompression,
            );
        }
    }
    let summary_msg = new_summary_message(summary_body, in_run);
    let mut preview_hist = history[..drop_start].to_vec();
    preview_hist.push(summary_msg.clone());
    preview_hist.extend(
        history[drop_start..drop_end]
            .iter()
            .filter(|m| splice.keep_ids.contains(&m.id))
            .cloned(),
    );
    preview_hist.extend(history[drop_end..].iter().cloned());
    let preview_for_disk = crate::conversation_store::conversation_preview(&preview_hist);
    if matches!(ui.scope, CompressionScope::Main) {
        crate::conversation_transcript::persist_compression_splice(
            conversation_id,
            &splice.excluded_for_persist,
            &summary_msg,
            &insert_before_message_id,
            &preview_for_disk,
        );
    } else if matches!(ui.scope, CompressionScope::SubAgent) {
        super::precompress::persist_sub_agent_compression_splice(
            conversation_id,
            ui,
            &splice.excluded_for_persist,
            &summary_msg,
            &insert_before_message_id,
        );
    }

    splice_summary_into_drop_window(
        history,
        drop_start,
        drop_end,
        &drop_ids,
        summary_msg.clone(),
    );

    // Strip images from remaining messages (belt-and-suspenders: also done in session.rs).
    // The verbatim tail may still carry base64 screenshots from computer agent rounds;
    // those payloads are wire-only and should not persist across turns.
    for m in history.iter_mut() {
        m.images_base64 = None;
        m.image_slot_labels = None;
    }

    if matches!(ui.scope, CompressionScope::Main) {
        crate::conversation_session::publish_working_set(conversation_id, history, None);
    }

    let messages_after = history.len();

    let compression = build_compression_info(
        ui,
        apply_reason,
        messages_before,
        messages_after,
        dropped_count,
        keep_users,
    );

    log::info!(
        "context_compress: applied conversation_id={} scope={:?} reason={} summary_failed={} messages_before={} messages_after={} in_run={} drop_start={} drop_end={} dropped={} kept_messages={} gate_tokens={} gate_source={} payload_est={} api_prompt={:?} budget_tokens={} summary_max_tokens={} summary_path={} summary_prompt_tokens={} summary_cached_tokens={} format_prefix_ms={} summary_llm_ms={} wall_ms={}",
        conversation_id,
        ui.scope,
        apply_reason,
        summary_failed,
        messages_before,
        messages_after,
        in_run,
        drop_start,
        drop_end,
        dropped_count,
        splice.keep_ids.len(),
        gate_tokens,
        gate_source,
        payload_est,
        api_prompt,
        budget_tokens,
        max_tok,
        if assembly.is_some() { "assembly" } else { "legacy" },
        summary_prompt_tokens,
        summary_cached_tokens,
        format_prefix_ms,
        t_llm.elapsed().as_millis(),
        wall.elapsed().as_millis()
    );

    let (done_msg, done_level) =
        compression_done_toast(ui, dropped_count, keep_users, summary_failed);
    if emit_compression_ui {
        emit_ui_toast(stream, conversation_id, &done_msg, done_level);
    }

    match ui.scope {
        CompressionScope::Main if emit_compression_ui => {
            crate::stream_broadcast::publish_stream(
                stream,
                StreamEvent::ContextCompressionApplied {
                    conversation_id: conversation_id.to_string(),
                    excluded_message_ids,
                    insert_before_message_id,
                    summary_message: summary_msg,
                    compression,
                },
            );
        }
        CompressionScope::SubAgent => {
            if let Some(message_id) = ui.message_id.as_deref() {
                crate::stream_broadcast::publish_stream(
                    stream,
                    StreamEvent::ContextCompressed {
                        conversation_id: conversation_id.to_string(),
                        message_id: message_id.to_string(),
                        compression,
                    },
                );
            } else {
                log::warn!(
                    "context_compress: sub_agent scope missing message_id conversation_id={}",
                    conversation_id
                );
            }
        }
        CompressionScope::Main => {}
    }
    true
}

pub(crate) fn clear_last_lead_prompt_tokens(conversation_id: &str) {
    if let Ok(store) = crate::conversation_store::global_store() {
        if let Err(e) = store.set_last_lead_prompt_tokens(conversation_id, None) {
            log::warn!(
                "conversation_store: clear last_lead_prompt_tokens after compression failed conversation_id={conversation_id}: {e}"
            );
        }
    }
}

/// When history exceeds char budget, summarize prefix. Emits `ContextCompressionApplied` when successful.
/// Returns whether durable history changed.
pub async fn maybe_compress_history(
    history: &mut Vec<ChatMessage>,
    settings: &ModelSettings,
    provider: &OpenAIProvider,
    conversation_id: &str,
    stream: &StreamTx,
    cancel: CancellationToken,
    ui: CompressionUiContext,
    memory_store: Option<&crate::memory::MemoryStore>,
    reported_prompt_tokens: Option<u32>,
) -> bool {
    let changed = compress_history_inner(
        history,
        settings,
        provider,
        conversation_id,
        stream,
        cancel,
        false,
        false,
        true,
        &ui,
        reported_prompt_tokens,
        None,
        false,
    )
    .await;
    if changed {
        if ui.scope == CompressionScope::Main {
            clear_last_lead_prompt_tokens(conversation_id);
            if let Some(store) = memory_store {
                if let Err(e) = store.reload_snapshot_for_conversation(conversation_id) {
                    log::warn!("memory: reload after compression failed: {e:#}");
                }
            }
        }
    }
    changed
}

/// After tool-round limit: try summarization even if under char budget. Returns whether history changed.
pub async fn maybe_compress_after_tool_round_limit(
    history: &mut Vec<ChatMessage>,
    settings: &ModelSettings,
    provider: &OpenAIProvider,
    conversation_id: &str,
    stream: &StreamTx,
    cancel: CancellationToken,
    emit_compression_ui: bool,
    ui: CompressionUiContext,
    reported_prompt_tokens: Option<u32>,
) -> bool {
    let changed = compress_history_inner(
        history,
        settings,
        provider,
        conversation_id,
        stream,
        cancel,
        true,
        false,
        emit_compression_ui,
        &ui,
        reported_prompt_tokens,
        None,
        false,
    )
    .await;
    if changed && ui.scope == CompressionScope::Main {
        clear_last_lead_prompt_tokens(conversation_id);
    }
    changed
}

/// Provider overflow: ignore the compressible-ratio gate, use a tighter token
/// tail, then summarize older turns if possible.
/// Intended to be called mid-loop so the same turn can continue.
pub async fn recover_history_after_overflow(
    history: &mut Vec<ChatMessage>,
    settings: &ModelSettings,
    provider: &OpenAIProvider,
    conversation_id: &str,
    stream: &StreamTx,
    cancel: CancellationToken,
    ui: CompressionUiContext,
    reported_prompt_tokens: Option<u32>,
) -> bool {
    let changed = compress_history_inner(
        history,
        settings,
        provider,
        conversation_id,
        stream,
        cancel,
        true,
        true,
        true,
        &ui,
        reported_prompt_tokens,
        None,
        false,
    )
    .await;
    if changed && ui.scope == CompressionScope::Main {
        clear_last_lead_prompt_tokens(conversation_id);
    }
    changed
}
