//! Host verify round orchestration (post desktop tool execute).

use crate::agents::computer::capture_debug;
use crate::agents::computer::pipeline::debug_ui::{
    emit_pipeline_phase_tool_complete, emit_pipeline_phase_tool_start,
};
use crate::agents::computer::pipeline::json_llm::{run_verify_llm, PipelineJsonLlmMeta};
use crate::agents::computer::pipeline::operation::{
    format_operation_summary, operation_family_for_tool,
};
use crate::agents::computer::pipeline::types::{
    PipelineLlmUsageRecorder, VerifyConclusion, VerifyModuleOutput,
};
use crate::agents::computer::pipeline::vision_pack::{
    build_verify_wire_messages, wire_messages_debug_text, VerifyWireInput,
};
use crate::agents::computer::tier::{ParsedVerify, PipelineLlmPhase, VerifyOutcome};
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

pub async fn run_verify_phase(
    computer_state: &ComputerState,
    conversation_id: &str,
    assistant_message_id: &str,
    provider: &OpenAIProvider,
    settings: &ModelSettings,
    root_tool_name: &str,
    root_args: &Value,
    before_cap: &ScreenCaptureResult,
    tool_result: Option<&str>,
    cancel: CancellationToken,
    dump_label: Option<&str>,
    file_prefix: &str,
    phase_label: &str,
    stream: Option<&StreamTx>,
    mut usage: Option<&mut PipelineLlmUsageRecorder<'_>>,
) -> Result<VerifyConclusion> {
    let after_cap = computer_state.capture_and_annotate(conversation_id).await?.0;
    let family = operation_family_for_tool(root_tool_name);
    let op_text = format!(
        "{} args={}",
        format_operation_summary(root_tool_name, root_args),
        root_args
    );
    let verify_prompt = computer_verify_prompt(family);
    let system = verify_prompt.trim().to_string();
    let verify_settings = computer_state.apply_pipeline_phase_settings(
        conversation_id,
        PipelineLlmPhase::Verify,
        settings,
    );
    let prov = OpenAIProvider::new(verify_settings.clone(), provider.api_key.clone());
    let mut after_for_cache = after_cap;
    let verify_wire = build_verify_wire_messages(&VerifyWireInput {
        family,
        before: before_cap,
        after: &after_for_cache,
        operation_text: &op_text,
        tool_result,
    });
    let wire_debug_text = wire_messages_debug_text(&verify_wire);
    let debug_ui = crate::llm_prompt_dump::pipeline_debug_ui_enabled(&verify_settings)
        .then(|| {
            stream.map(|tx| {
                emit_pipeline_phase_tool_start(
                    tx,
                    assistant_message_id,
                    "verify",
                    root_tool_name,
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
                before_cap.raw_marked_jpeg.as_slice(),
            ),
            (
                SLOT_SCREEN_AFTER_ACTION,
                "jpg",
                after_for_cache.raw_marked_jpeg.as_slice(),
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
                root_tool_name,
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
        after_for_cache = match computer_state.capture_and_annotate(conversation_id).await {
            Ok((cap, _)) => cap,
            Err(err) => {
                emit_verify_debug_complete(
                    stream,
                    debug_ui.as_ref(),
                    assistant_message_id,
                    root_tool_name,
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
            &after_for_cache.annotated_marked_jpeg,
        );
        verify_wire = build_verify_wire_messages(&VerifyWireInput {
            family,
            before: before_cap,
            after: &after_for_cache,
            operation_text: &op_text,
            tool_result,
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
                    root_tool_name,
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
                root_tool_name,
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
        root_tool_name,
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
}
