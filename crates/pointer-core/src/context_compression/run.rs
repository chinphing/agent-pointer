//! Apply a compression plan to live history (sync paths).

use super::budget::*;
use super::precompress::{enqueue_pending_splice, PendingCompressionSplice};
use super::summary::*;
use super::types::*;
use crate::models::{ChatMessage, ModelSettings, Role, StreamEvent};
use crate::provider::OpenAIProvider;
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

pub(crate) async fn compress_history_inner(
    history: &mut Vec<ChatMessage>,
    settings: &ModelSettings,
    provider: &OpenAIProvider,
    conversation_id: &str,
    stream: &StreamTx,
    cancel: CancellationToken,
    force_ignore_char_budget: bool,
    overflow_recover: bool,
    // When true (background precompress), compress once soft gate + ratio pass
    // even if still under the hard budget.
    soft_precompress: bool,
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
    if !settings.context_compression_enabled {
        log::info!(
            "context_compress: skip_disabled conversation_id={} messages={} wall_ms={}",
            conversation_id,
            messages_before,
            wall.elapsed().as_millis()
        );
        return false;
    }
    let keep_users = settings.context_keep_recent_user_turns.max(1);
    let budget_tokens = normalize_context_budget_tokens(settings.context_budget_tokens);

    let gate = evaluate_compress_gate(
        history,
        reported_prompt_tokens,
        budget_tokens,
        keep_users as usize,
        soft_precompress,
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
        soft_precompress,
        overflow_recover,
        force_ignore_char_budget,
    );
    let (drop_start, drop_end, in_run) = match plan {
        CompressionPlan::Skip => {
            log::debug!(
                "context_compress: skip_gate conversation_id={} soft={} total={} prefix={} ratio={:.3} threshold={} split={} share={:.3} budget_tokens={} wall_ms={}",
                conversation_id,
                soft_precompress,
                gate.total,
                gate.prefix,
                gate.ratio,
                gate.threshold,
                gate.split,
                current_turn_message_share(history),
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

    // In-thread marker at the summary split (frontend); completion still uses UiToast.
    if emit_compression_ui {
        let insert_before = history
            .get(drop_end)
            .map(|m| m.id.clone())
            .filter(|id| !id.trim().is_empty());
        emit_compression_started(stream, conversation_id, ui, insert_before);
    }

    let dropped_count = history[drop_start..drop_end]
        .iter()
        .filter(|m| {
            crate::message_context::is_context_included(m)
                && !crate::message_context::is_synthetic_user_content(&m.content)
        })
        .count() as u32;
    let t_fmt = Instant::now();
    let formatted = format_prefix_for_summary(summary_source);
    let format_prefix_ms = t_fmt.elapsed().as_millis();
    let content_tokens = estimate_text_tokens_heuristic(&formatted);
    let max_tok = compute_summary_max_tokens(content_tokens);
    // Provider cap gets 1.3× headroom over the target budget so the model can
    // close out without `finish_reason=length` (mirrors Hermes). The budget
    // itself stays the prompt target (~N tokens) and the log label.
    let requested_max_tok = summary_max_tokens_requested(max_tok);
    let summary_user_prompt = build_summary_user_prompt(&formatted, max_tok, in_run);
    let input = ChatMessage {
        id: format!("sum_in_{}", uuid::Uuid::new_v4().simple()),
        role: Role::User,
        content: summary_user_prompt,
        status: "done".into(),
        created_at: now_ms(),
        tool_calls: None,
        tool_call_id: None,
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
    let dump_lbl = format!(
        "{}_context_summary_{}",
        conversation_id, reason
    );
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
    let summary_system = build_summary_system_prompt(ui, in_run);
    let summary_sections =
        crate::models::SystemPromptSections::all_cacheable(vec![summary_system.clone()]);
    // First attempt: no-thinking + 20% content budget (ceiling 12k), provider
    // cap gets 1.3× headroom (mirrors Hermes). A `finish_reason=length`
    // rejection is retried once at budget×2 before falling through to
    // drop_without_summary.
    let summary_text = match provider
        .chat_once_without_thinking(
            std::slice::from_ref(&input),
            &summary_sections,
            Vec::new(),
            cancel.clone(),
            Some(requested_max_tok),
            Some(dump_lbl.as_str()),
        )
        .await
    {
        Ok(out) => {
            record_summary_usage(
                ui,
                &out,
                "no_thinking",
                crate::llm_token_stats::active_provider_source(&provider.settings),
            );
            match validate_summary_output(&out) {
                Ok(text) => Some(text),
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
                        let retry_max = summary_max_tokens_retry(max_tok);
                        log::info!(
                            "context summary retrying conversation_id={} attempt=retry_length budget={} requested={} summary_llm_ms={}",
                            conversation_id,
                            max_tok,
                            retry_max,
                            t_llm.elapsed().as_millis(),
                        );
                        match provider
                            .chat_once_without_thinking(
                                std::slice::from_ref(&input),
                                &summary_sections,
                                Vec::new(),
                                cancel.clone(),
                                Some(retry_max),
                                Some(dump_lbl.as_str()),
                            )
                            .await
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
                                match validate_summary_output(&out2) {
                                    Ok(text) => Some(text),
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
                                        None
                                    }
                                }
                            }
                            Err(e) => {
                                log::warn!(
                                    "context summary LLM call failed conversation_id={} attempt=retry_length error={e:#} budget={} requested={} summary_llm_ms={}",
                                    conversation_id,
                                    max_tok,
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

    let mut summary_failed = false;
    if cancel.is_cancelled() {
        log::info!(
            "context_compress: cancelled after LLM summary, aborting splice conversation_id={} wall_ms={}",
            conversation_id,
            wall.elapsed().as_millis()
        );
        return false;
    }

    let summary_body = match summary_text {
        Some(text) => build_persisted_summary(summary_prefix, &text),
        None => {
            // Hermes default: drop the middle window with a deterministic handoff
            // instead of leaving an over-budget transcript unchanged.
            summary_failed = true;
            log::warn!(
                "context_compress: drop_without_summary conversation_id={} scope={:?} messages={} in_run={} drop_start={} drop_end={} dropped={} wall_ms={}",
                conversation_id,
                ui.scope,
                messages_before,
                in_run,
                drop_start,
                drop_end,
                dropped_count,
                wall.elapsed().as_millis()
            );
            build_drop_without_summary_body(summary_prefix, dropped_count)
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

    let insert_before_message_id = history
        .get(drop_end)
        .map(|m| m.id.clone())
        .unwrap_or_default();
    let fingerprint_prefix_ids: Vec<String> = history[drop_start..drop_end]
        .iter()
        .map(|m| m.id.clone())
        .collect();

    let turn_busy = force_enqueue
        || (matches!(ui.scope, CompressionScope::Main)
            && defer_persist_state
                .as_ref()
                .map(|s| s.cancels.lock().contains_key(conversation_id))
                .unwrap_or(false));
    if turn_busy {
        let mut excluded_for_persist: Vec<ChatMessage> = history[drop_start..drop_end]
            .iter()
            .filter(|m| crate::message_context::is_context_included(m))
            .cloned()
            .collect();
        for m in &mut excluded_for_persist {
            crate::message_context::mark_excluded(
                m,
                crate::models::ExcludedReason::ContextCompression,
            );
        }
        let summary_msg = new_summary_message(summary_body, in_run);
        let mut preview_hist = history[..drop_start].to_vec();
        preview_hist.push(summary_msg.clone());
        preview_hist.extend(history[drop_end..].iter().cloned());
        let preview_for_disk = crate::conversation_store::conversation_preview(&preview_hist);
        enqueue_pending_splice(PendingCompressionSplice {
            queue_key: compression_queue_key(conversation_id, ui),
            conversation_id: conversation_id.to_string(),
            ui: ui.clone(),
            fingerprint_prefix_ids,
            insert_before_message_id,
            excluded_for_persist,
            summary_msg,
            preview_for_disk,
            dropped_count,
            keep_users,
            apply_reason: apply_reason.to_string(),
            summary_failed,
        });
        log::info!(
            "context_compress: enqueued splice (turn busy) conversation_id={} in_run={} drop_start={} drop_end={} dropped={} share={:.3} wall_ms={}",
            conversation_id,
            in_run,
            drop_start,
            drop_end,
            dropped_count,
            current_turn_message_share(history),
            wall.elapsed().as_millis()
        );
        return false;
    }

    let excluded_message_ids =
        mark_compressed_prefix_excluded(&mut history[drop_start..drop_end]);
    let excluded_for_persist: Vec<ChatMessage> = history[drop_start..drop_end]
        .iter()
        .filter(|m| excluded_message_ids.iter().any(|id| id == &m.id))
        .cloned()
        .collect();
    let summary_msg = new_summary_message(summary_body, in_run);
    history.insert(drop_end, summary_msg.clone());

    // Persist before drain: soft-exclude payloads + shift suffix + insert summary.
    let mut preview_hist = history[..drop_start].to_vec();
    preview_hist.push(summary_msg.clone());
    preview_hist.extend(history[(drop_end + 1)..].iter().cloned());
    let preview_for_disk = crate::conversation_store::conversation_preview(&preview_hist);
    if matches!(ui.scope, CompressionScope::Main) {
        crate::conversation_transcript::persist_compression_splice(
            conversation_id,
            &excluded_for_persist,
            &summary_msg,
            &insert_before_message_id,
            &preview_for_disk,
        );
    }

    // After insert, dropped rows are still [drop_start, drop_end) and the
    // summary sits at drop_end. Drain the dropped window so the summary
    // slides into place (prefix path: drop_start=0).
    history.drain(drop_start..drop_end);

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
        "context_compress: applied conversation_id={} scope={:?} reason={} summary_failed={} messages_before={} messages_after={} in_run={} drop_start={} drop_end={} gate_tokens={} gate_source={} payload_est={} api_prompt={:?} budget_tokens={} summary_max_tokens={} format_prefix_ms={} summary_llm_ms={} wall_ms={}",
        conversation_id,
        ui.scope,
        apply_reason,
        summary_failed,
        messages_before,
        messages_after,
        in_run,
        drop_start,
        drop_end,
        gate_tokens,
        gate_source,
        payload_est,
        api_prompt,
        budget_tokens,
        max_tok,
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
        false,
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

