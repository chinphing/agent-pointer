//! Host verify round orchestration (post desktop tool execute).

use crate::agents::computer::capture_debug;
use crate::agents::computer::pipeline::debug_ui::{
    emit_pipeline_phase_tool_complete, emit_pipeline_phase_tool_start,
};
use crate::agents::computer::pipeline::json_llm::{run_verify_llm, PipelineJsonLlmMeta};
use crate::agents::computer::pipeline::operation::{
    format_operation_summary, operation_family_for_tool, OperationFamily,
};
use crate::agents::computer::pipeline::types::{
    PipelineLlmUsageRecorder, VerifyConclusion, VerifyModuleOutput,
};
use crate::agents::computer::pipeline::vision_pack::{
    build_verify_wire_messages, wire_messages_debug_text, VerifyWireInput,
};
use crate::agents::computer::runtime::{ParsedVerify, PipelineLlmPhase, VerifyOutcome};
use crate::agents::computer::vision::screen_overlay::{
    SLOT_SCREEN_AFTER_ACTION, SLOT_SCREEN_BEFORE_ACTION,
};
use crate::agents::computer::ComputerState;
use crate::agents::computer::ScreenCaptureResult;
use crate::agents::computer_verify_prompt;
use crate::chat_service::StreamTx;
use crate::models::{ModelSettings, ToolCall};
use crate::provider::OpenAIProvider;
use crate::task_board::checkpoint::is_task_board_tool_name;
use crate::tools::ToolRegistry;
use anyhow::Result;
use serde_json::Value;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

const PIPELINE_LOADING_WAIT_SECS: f64 = 2.5;
const PIPELINE_LOADING_MAX_LOOPS: u32 = 4;

fn should_retry_verify_for_loading(out: &VerifyModuleOutput, loops_done: u32) -> bool {
    out.loading_detected && loops_done < PIPELINE_LOADING_MAX_LOOPS
}

/// One desktop tool in the batch that verify has to judge.
///
/// A turn may emit several desktop tools. They are judged together: verify sees
/// the frame before the batch and the frame after it, so the batch has to be
/// something those two frames can prove — same goal, or targets that cannot
/// affect each other.
#[derive(Debug, Clone)]
pub struct VerifyTarget {
    pub name: String,
    pub args: Value,
    /// Tool result text for this call, when the host captured one.
    pub tool_result: Option<String>,
}

impl VerifyTarget {
    pub fn new(name: impl Into<String>, args: Value, tool_result: Option<String>) -> Self {
        Self {
            name: name.into(),
            args,
            tool_result,
        }
    }
}

/// Prompt family for a batch: one family when every call shares it, else the
/// generic prompt — a mixed batch (click then type) matches no single family's
/// success criteria.
fn batch_family(targets: &[VerifyTarget]) -> OperationFamily {
    let mut families = targets.iter().map(|t| operation_family_for_tool(&t.name));
    let Some(first) = families.next() else {
        return OperationFamily::Other;
    };
    if families.all(|f| f == first) {
        first
    } else {
        OperationFamily::Other
    }
}

/// Human-readable list of the batch, one line per call, numbered when there is
/// more than one so verify can refer to "call 2".
fn batch_operation_text(targets: &[VerifyTarget]) -> String {
    if targets.len() == 1 {
        let t = &targets[0];
        return format!(
            "{} args={}",
            format_operation_summary(&t.name, &t.args),
            t.args
        );
    }
    let mut out = format!(
        "{} desktop calls in this batch, executed in order. The before frame is \
         the screen before call 1; the after frame is the screen after call {}.\n",
        targets.len(),
        targets.len()
    );
    for (i, t) in targets.iter().enumerate() {
        out.push_str(&format!(
            "{}. {} args={}\n",
            i + 1,
            format_operation_summary(&t.name, &t.args),
            t.args
        ));
        if let Some(text) = t.tool_result.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            out.push_str(&format!("   result: {text}\n"));
        }
    }
    out
}

pub async fn run_verify_phase(
    computer_state: &ComputerState,
    conversation_id: &str,
    assistant_message_id: &str,
    settings: &ModelSettings,
    targets: &[VerifyTarget],
    before_cap: &ScreenCaptureResult,
    cancel: CancellationToken,
    dump_label: Option<&str>,
    file_prefix: &str,
    phase_label: &str,
    stream: Option<&StreamTx>,
    mut usage: Option<&mut PipelineLlmUsageRecorder<'_>>,
) -> Result<VerifyConclusion> {
    let after_cap = computer_state
        .capture_screen(conversation_id)
        .await?
        .0;
    // Read the pointer once, right after the after-frame capture: it sits on the
    // click point, and both frames then show the same spot. A pointer that moved
    // later (user interference) is not something this marker can fix anyway.
    let current_pointer = crate::agents::computer::vision::screen::current_pointer().ok();
    let family = batch_family(targets);
    let op_text = batch_operation_text(targets);
    // Debug/card label. A batch has no single tool name, so name it by size.
    let debug_label = match targets {
        [only] => only.name.clone(),
        many => format!("batch of {}", many.len()),
    };
    let tool_result = targets
        .iter()
        .find_map(|t| t.tool_result.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let verify_prompt = computer_verify_prompt(family);
    let system = verify_prompt.trim().to_string();
    let verify_settings = computer_state.apply_pipeline_phase_settings(
        conversation_id,
        PipelineLlmPhase::Verify,
        settings,
    );
    // Verify may point at a different provider than the agent — resolve the key
    // for that provider instead of reusing the agent's (a mismatch here shows up
    // as 401 from the wrong endpoint).
    let verify_key = crate::provider::api_key_for_settings(&verify_settings)?;
    let prov = OpenAIProvider::new(verify_settings.clone(), verify_key);
    let mut after_for_cache = after_cap;
    let verify_wire = build_verify_wire_messages(&VerifyWireInput {
        family,
        before: before_cap,
        after: &after_for_cache,
        operation_text: &op_text,
        tool_result,
        current_pointer,
    });
    let wire_debug_text = wire_messages_debug_text(&verify_wire);
    let debug_ui = crate::llm_prompt_dump::pipeline_debug_ui_enabled(&verify_settings)
        .then(|| {
            stream.map(|tx| {
                emit_pipeline_phase_tool_start(
                    tx,
                    assistant_message_id,
                    "verify",
                    &debug_label,
                    &wire_debug_text,
                )
            })
        })
        .flatten();
    let verify_started = Instant::now();

    capture_debug::save_pipeline_phase_captures(
        conversation_id,
        file_prefix,
        phase_label,
        &[
            (
                SLOT_SCREEN_BEFORE_ACTION,
                "jpg",
                before_cap.screenshot_jpeg.as_slice(),
            ),
            (
                SLOT_SCREEN_AFTER_ACTION,
                "jpg",
                after_for_cache.screenshot_jpeg.as_slice(),
            ),
        ],
    );
    let mut verify_wire = verify_wire;
    let (mut out, mut last_meta) = match run_verify_once(
        &prov,
        &verify_settings,
        &system,
        verify_wire.clone(),
        cancel.clone(),
        dump_label,
        usage.as_deref_mut(),
    )
    .await
    {
        Ok(bundle) => (bundle.output, bundle.meta),
        Err(err) => {
            emit_verify_debug_complete(
                stream,
                debug_ui.as_ref(),
                assistant_message_id,
                &debug_label,
                verify_started,
                false,
                None,
                "",
                Some(&err.to_string()),
            );
            return Err(err);
        }
    };

    let mut loading_loops = 0u32;
    while should_retry_verify_for_loading(&out, loading_loops) {
        loading_loops += 1;
        log::info!(
            "computer pipeline: loading_detected loop {loading_loops}/{PIPELINE_LOADING_MAX_LOOPS} — waiting {PIPELINE_LOADING_WAIT_SECS}s before re-verify conversation_id={conversation_id}"
        );
        tokio::time::sleep(tokio::time::Duration::from_secs_f64(
            PIPELINE_LOADING_WAIT_SECS,
        ))
        .await;
        after_for_cache = match computer_state.capture_screen(conversation_id).await {
            Ok((cap, _)) => cap,
            Err(err) => {
                emit_verify_debug_complete(
                    stream,
                    debug_ui.as_ref(),
                    assistant_message_id,
                    &debug_label,
                    verify_started,
                    false,
                    last_meta.reasoning.as_deref(),
                    &last_meta.raw_text,
                    Some(&err.to_string()),
                );
                return Err(err);
            }
        };
        capture_debug::save_pipeline_slot_image(
            conversation_id,
            file_prefix,
            phase_label,
            SLOT_SCREEN_AFTER_ACTION,
            "jpg",
            &after_for_cache.screenshot_jpeg,
        );
        verify_wire = build_verify_wire_messages(&VerifyWireInput {
            family,
            before: before_cap,
            after: &after_for_cache,
            operation_text: &op_text,
            tool_result,
            current_pointer,
        });
        match run_verify_once(
            &prov,
            &verify_settings,
            &system,
            verify_wire,
            cancel.clone(),
            dump_label,
            usage.as_deref_mut(),
        )
        .await
        {
            Ok(bundle) => {
                out = bundle.output;
                last_meta = bundle.meta;
            }
            Err(err) => {
                emit_verify_debug_complete(
                    stream,
                    debug_ui.as_ref(),
                    assistant_message_id,
                    &debug_label,
                    verify_started,
                    false,
                    last_meta.reasoning.as_deref(),
                    &last_meta.raw_text,
                    Some(&err.to_string()),
                );
                return Err(err);
            }
        }
    }
    if out.loading_detected {
        log::warn!(
            "computer pipeline: loading still detected after {loading_loops} wait+re-verify loop(s); using last verify output conversation_id={conversation_id}"
        );
    }

    let conclusion = match VerifyConclusion::from_module_output(&out) {
        Ok(c) => c,
        Err(err) => {
            let output_text = serde_json::to_string_pretty(&out).unwrap_or_default();
            emit_verify_debug_complete(
                stream,
                debug_ui.as_ref(),
                assistant_message_id,
                &debug_label,
                verify_started,
                false,
                last_meta.reasoning.as_deref(),
                &output_text,
                Some(&err.to_string()),
            );
            return Err(err);
        }
    };

    emit_verify_debug_complete(
        stream,
        debug_ui.as_ref(),
        assistant_message_id,
        &debug_label,
        verify_started,
        true,
        last_meta.reasoning.as_deref(),
        last_meta.raw_text.as_str(),
        None,
    );

    computer_state.apply_pipeline_verify_result(conversation_id, &conclusion);
    computer_state.store_pipeline_after_capture(conversation_id, after_for_cache);
    Ok(conclusion)
}

struct VerifyOnceBundle {
    output: VerifyModuleOutput,
    meta: PipelineJsonLlmMeta,
}

fn emit_verify_debug_complete(
    stream: Option<&StreamTx>,
    debug_ui: Option<&super::debug_ui::PipelinePhaseToolEmit>,
    assistant_message_id: &str,
    root_tool_name: &str,
    verify_started: Instant,
    ok: bool,
    reasoning: Option<&str>,
    output: &str,
    error: Option<&str>,
) {
    if let (Some(tx), Some(emit_ctx)) = (stream, debug_ui) {
        emit_pipeline_phase_tool_complete(
            tx,
            assistant_message_id,
            emit_ctx,
            root_tool_name,
            ok,
            reasoning,
            output,
            error,
            verify_started.elapsed().as_millis() as u64,
        );
    }
}

async fn run_verify_once(
    prov: &OpenAIProvider,
    settings: &ModelSettings,
    system: &str,
    wire: Vec<Value>,
    cancel: CancellationToken,
    dump_label: Option<&str>,
    usage: Option<&mut PipelineLlmUsageRecorder<'_>>,
) -> Result<VerifyOnceBundle> {
    let (output, meta) =
        run_verify_llm(prov, settings, system, wire, cancel, dump_label, usage).await?;
    Ok(VerifyOnceBundle { output, meta })
}

fn find_root_desktop_tool<'a>(
    calls: &'a [ToolCall],
    tools: &ToolRegistry,
) -> Option<(String, Value)> {
    for tc in calls {
        let name = tc.name.trim();
        if is_task_board_tool_name(name) {
            continue;
        }
        if tools.get_def(name).is_some() {
            let args = serde_json::from_str(&tc.arguments).unwrap_or(Value::Null);
            return Some((name.to_string(), args));
        }
    }
    None
}

pub fn find_root_tool_name(calls: &[ToolCall], tools: &ToolRegistry) -> Option<String> {
    find_root_desktop_tool(calls, tools).map(|(n, _)| n)
}

pub fn verify_outcome_for_tier(conclusion: &VerifyConclusion) -> ParsedVerify {
    ParsedVerify {
        step_result: conclusion.action_result.as_history_str().to_string(),
        cause: conclusion
            .failure_cause
            .as_ref()
            .map(|c| c.as_str().to_string()),
    }
}

pub fn verify_outcome_struct(conclusion: &VerifyConclusion) -> VerifyOutcome {
    VerifyOutcome {
        step_result: conclusion.action_result.as_history_str().to_string(),
        cause: conclusion
            .failure_cause
            .as_ref()
            .map(|c| c.as_str().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_retry_loop_guard() {
        let loading = VerifyModuleOutput {
            action_result: "pending".into(),
            failure_cause: None,
            step_summary: None,
            loading_detected: true,
        };
        let clear = VerifyModuleOutput {
            loading_detected: false,
            ..loading.clone()
        };
        assert!(should_retry_verify_for_loading(&loading, 0));
        assert!(should_retry_verify_for_loading(
            &loading,
            PIPELINE_LOADING_MAX_LOOPS - 1
        ));
        assert!(!should_retry_verify_for_loading(
            &loading,
            PIPELINE_LOADING_MAX_LOOPS
        ));
        assert!(!should_retry_verify_for_loading(&clear, 0));
    }

    #[test]
    fn batch_family_is_generic_when_calls_mix_families() {
        // A batch of click + type matches no single family's success criteria, so
        // it falls back to the generic prompt instead of judging a type as a click.
        let click = VerifyTarget::new("mouse_click_at", Value::Null, None);
        let input = VerifyTarget::new("input_at", Value::Null, None);
        assert_eq!(batch_family(&[click.clone()]), OperationFamily::PointerClick);
        assert_eq!(batch_family(&[input.clone()]), OperationFamily::Input);
        assert_eq!(batch_family(&[click, input]), OperationFamily::Other);
    }

    #[test]
    fn batch_operation_text_lists_every_call() {
        // Verify has to see all of them: judging only the first left the rest
        // unverified while the conclusion still landed on a history row.
        let one = VerifyTarget::new("mouse_click_at", Value::Null, None);
        assert_eq!(batch_operation_text(&[one]).lines().count(), 1);

        let batch = vec![
            VerifyTarget::new("mouse_click_at", Value::Null, Some("ok".into())),
            VerifyTarget::new("input_at", Value::Null, Some("typed".into())),
            VerifyTarget::new("hotkey", Value::Null, None),
        ];
        let text = batch_operation_text(&batch);
        assert!(text.contains("3 desktop calls in this batch"));
        assert!(text.contains("1. "));
        assert!(text.contains("2. "));
        assert!(text.contains("3. "));
        assert!(text.contains("result: ok"));
        assert!(text.contains("result: typed"));
    }
}
