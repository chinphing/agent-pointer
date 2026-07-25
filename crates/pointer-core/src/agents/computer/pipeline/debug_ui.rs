//! Debug-mode UI: surface pipeline Position / Verify phases as synthetic tool-call cards.

use crate::chat_service::StreamTx;
use crate::models::{StreamEvent, ToolCall};
use crate::stream_broadcast::publish_stream;
use serde_json::json;

const TOOL_NAME_POSITION: &str = "computer_pipeline_position";
const TOOL_NAME_VERIFY: &str = "computer_pipeline_verify";

pub struct PipelinePhaseToolEmit {
    pub tool_call_id: String,
    phase: &'static str,
}

fn tool_name_for_phase(phase: &str) -> &'static str {
    match phase {
        "verify" => TOOL_NAME_VERIFY,
        _ => TOOL_NAME_POSITION,
    }
}

fn display_label_for_phase(phase: &str) -> &'static str {
    match phase {
        "verify" => "流水线 · 校验",
        _ => "流水线 · 定位",
    }
}

fn build_args(wire_input: &str) -> String {
    serde_json::to_string(&json!({ "input": wire_input.trim() })).unwrap_or_else(|_| "{}".into())
}

fn build_result(reasoning: Option<&str>, output: &str) -> String {
    let reasoning = reasoning
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    serde_json::to_string(&json!({
        "reasoning": reasoning,
        "output": output.trim(),
    }))
    .unwrap_or_else(|_| output.trim().to_string())
}

/// Emit `tool_call_start` for a pipeline LLM phase (debug mode only).
pub fn emit_pipeline_phase_tool_start(
    stream: &StreamTx,
    assistant_message_id: &str,
    phase: &str,
    target_tool: &str,
    wire_input: &str,
) -> PipelinePhaseToolEmit {
    let tool_call_id =
        crate::extensions::new_extension_message_id(&format!("pipeline_{phase}_{target_tool}"));
    let name = tool_name_for_phase(phase);
    let display_summary = target_tool.to_string();
    publish_stream(
        stream,
        StreamEvent::ToolCallStart {
            message_id: assistant_message_id.to_string(),
            tool_call: ToolCall {
                id: tool_call_id.clone(),
                name: name.into(),
                arguments: build_args(wire_input),
                status: "running".into(),
                result: None,
                error: None,
                duration_ms: None,
                risk_level: Some("low".into()),
                display_label: Some(display_label_for_phase(phase).into()),
                display_summary: Some(display_summary),
            },
            trace_id: None,
            scoped_message_id: None,
        },
    );
    PipelinePhaseToolEmit {
        tool_call_id,
        phase: if phase == "verify" {
            "verify"
        } else {
            "position"
        },
    }
}

/// Emit `tool_call_status` when a pipeline LLM phase completes.
pub fn emit_pipeline_phase_tool_complete(
    stream: &StreamTx,
    assistant_message_id: &str,
    emit_ctx: &PipelinePhaseToolEmit,
    target_tool: &str,
    ok: bool,
    reasoning: Option<&str>,
    output: &str,
    error: Option<&str>,
    duration_ms: u64,
) {
    let result = if ok {
        Some(build_result(reasoning, output))
    } else {
        None
    };
    publish_stream(
        stream,
        StreamEvent::ToolCallStatus {
            message_id: assistant_message_id.to_string(),
            tool_call_id: emit_ctx.tool_call_id.clone(),
            status: if ok {
                "success".into()
            } else {
                "failed".into()
            },
            result,
            error: error.map(str::to_string),
            duration_ms: Some(duration_ms),
            display_label: Some(display_label_for_phase(emit_ctx.phase).into()),
            display_summary: Some(target_tool.to_string()),
            trace_id: None,
            scoped_message_id: None,
        },
    );
    log::info!(
        "computer pipeline: debug UI tool card phase={} target={target_tool} ok={ok} duration_ms={duration_ms}",
        emit_ctx.phase
    );
}
