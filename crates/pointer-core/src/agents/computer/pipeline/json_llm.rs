//! Pipeline Verify LLM calls — virtual submit_verify tool + thinking.

use crate::llm_token_stats::model_name_for_usage_report;
use crate::models::ModelSettings;
use crate::provider::{ChatOnceOutput, OpenAIProvider};
use anyhow::{anyhow, Result};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::module_tools::{parse_verify_from_tool_calls, tool_call_raw_text, verify_tools};
use super::types::PipelineLlmUsageRecorder;

/// Raw LLM response metadata for pipeline debug UI (no system prompt).
#[derive(Debug, Clone)]
pub struct PipelineJsonLlmMeta {
    pub raw_text: String,
    pub reasoning: Option<String>,
}

fn record_chat_once_usage(
    recorder: Option<&mut PipelineLlmUsageRecorder<'_>>,
    out: &ChatOnceOutput,
) {
    let Some(r) = recorder else {
        return;
    };
    r.stats.record_llm_round(
        r.scope,
        out.usage.as_ref(),
        model_name_for_usage_report(&out.model),
    );
}

async fn chat_pipeline_module_with_tools(
    provider: &OpenAIProvider,
    settings: &ModelSettings,
    system_text: &str,
    wire_messages: Vec<Value>,
    tools: Vec<Value>,
    phase: &str,
    cancel: CancellationToken,
    dump_label: Option<&str>,
    usage: Option<&mut PipelineLlmUsageRecorder<'_>>,
) -> Result<(ChatOnceOutput, PipelineJsonLlmMeta)> {
    if crate::llm_prompt_dump::should_dump(settings) {
        let wire_text = sanitize_wire_messages_for_log(&wire_messages);
        log::info!(
            "[PIPELINE_LLM] === {} PROMPT ===\n{}\n\n=== WIRE MESSAGES ===\n{}\n=== END {} PROMPT ===",
            phase.to_uppercase(),
            system_text,
            wire_text,
            phase.to_uppercase()
        );
    }

    let out = provider
        .stream_pipeline_module_with_tools(
            wire_messages,
            Some(system_text),
            tools,
            cancel,
            None,
            dump_label,
        )
        .await?;

    if crate::llm_prompt_dump::should_dump(settings) {
        log::info!(
            "[PIPELINE_LLM] === {} OUTPUT tool_calls={} reasoning_chars={} ===\n{:?}\n=== END {} OUTPUT ===",
            phase.to_uppercase(),
            out.tool_calls.len(),
            out.reasoning_content.as_ref().map(|s| s.chars().count()).unwrap_or(0),
            out.tool_calls,
            phase.to_uppercase()
        );
    }

    record_chat_once_usage(usage, &out);
    let raw_text = out
        .tool_calls
        .first()
        .map(tool_call_raw_text)
        .unwrap_or_default();
    let meta = PipelineJsonLlmMeta {
        raw_text: raw_text.clone(),
        reasoning: out.reasoning_content.clone(),
    };
    crate::logging::write_pipeline_llm_segments_to_stderr(
        "pipeline/verify",
        meta.reasoning.as_deref(),
        &meta.raw_text,
    );
    if out.tool_calls.is_empty() {
        return Err(anyhow!(
            "pipeline {phase}: no submit tool call (reasoning_chars={} content_chars={})",
            meta.reasoning
                .as_ref()
                .map(|s| s.chars().count())
                .unwrap_or(0),
            out.text.chars().count()
        ));
    }
    Ok((out, meta))
}

pub async fn run_verify_llm(
    provider: &OpenAIProvider,
    settings: &ModelSettings,
    system_text: &str,
    wire_messages: Vec<Value>,
    cancel: CancellationToken,
    dump_label: Option<&str>,
    usage: Option<&mut PipelineLlmUsageRecorder<'_>>,
) -> Result<(super::types::VerifyModuleOutput, PipelineJsonLlmMeta)> {
    let (out, meta) = chat_pipeline_module_with_tools(
        provider,
        settings,
        system_text,
        wire_messages,
        verify_tools(),
        "verify",
        cancel,
        dump_label,
        usage,
    )
    .await?;
    let verify = parse_verify_from_tool_calls(&out.tool_calls)?;
    Ok((verify, meta))
}

/// Strip base64 image data from wire messages before logging.
fn sanitize_wire_messages_for_log(messages: &[serde_json::Value]) -> String {
    fn strip_image_base64(val: &mut serde_json::Value) {
        match val {
            serde_json::Value::Object(map) => {
                if let Some(url) = map.get("url").and_then(|v| v.as_str()) {
                    if url.starts_with("data:image/") {
                        let len = url.len();
                        *map.get_mut("url").unwrap() =
                            serde_json::Value::String(format!("[IMAGE_BASE64: {len} bytes]"));
                        return;
                    }
                }
                for v in map.values_mut() {
                    strip_image_base64(v);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr.iter_mut() {
                    strip_image_base64(v);
                }
            }
            _ => {}
        }
    }

    let mut cloned: Vec<serde_json::Value> = messages.iter().cloned().collect();
    for msg in &mut cloned {
        strip_image_base64(msg);
    }
    serde_json::to_string_pretty(&cloned).unwrap_or_else(|_| "<serialization error>".to_string())
}
