//! Shared host verify hooks for single-agent and sub-agent loops.

use crate::agents::computer::pipeline::operation::{
    format_operation_summary, needs_verify_for_tool, operation_family_for_tool, OperationFamily,
};
use crate::agents::computer::pipeline::types::{ActionResult, FailureCause, VerifyConclusion};
pub use crate::agents::computer::pipeline::PipelineLlmUsageRecorder;
use crate::agents::computer::pipeline::{find_root_tool_name, run_verify_phase};
use crate::agents::computer::ScreenCaptureResult;
use crate::agents::AgentProfile;
use crate::chat_service::app_state::AppState;
use crate::chat_service::emit::emit;
use crate::chat_service::StreamTx;
use crate::models::{ChatMessage, ModelSettings, Role, StreamEvent, ToolCall};
use crate::provider::OpenAIProvider;
use crate::tools::ToolRegistry;
use anyhow::{anyhow, Result};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

pub fn verify_host_active(state: &AppState, profile: &AgentProfile) -> bool {
    *profile == AgentProfile::Computer && state.computer_state.verify_host_enabled()
}

pub fn batch_has_desktop_root_tool(tool_calls: &[ToolCall], tools: &ToolRegistry) -> bool {
    find_root_tool_name(tool_calls, tools).is_some()
}

pub async fn ensure_verify_before_capture(
    state: &AppState,
    conversation_id: &str,
) -> Result<ScreenCaptureResult> {
    let _round = state
        .computer_state
        .next_pipeline_round_seq(conversation_id);
    state
        .computer_state
        .ensure_pipeline_capture(conversation_id)
        .await
}

pub fn tool_result_text_for_call(
    history: &[ChatMessage],
    message_id: &str,
    tool_call_id: &str,
) -> Option<String> {
    let idx = history.iter().position(|m| m.id == message_id)?;
    let mut j = idx + 1;
    while j < history.len() && matches!(history[j].role, Role::Tool) {
        if history[j].tool_call_id.as_deref() == Some(tool_call_id) {
            let content = history[j].content.trim();
            if !content.is_empty() {
                return Some(content.to_string());
            }
        }
        j += 1;
    }
    None
}

fn is_internal_tool_log_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("[tool:") || t.starts_with("[args]") || t.starts_with("[output]")
}

fn host_verify_conclusion(
    family: OperationFamily,
    root_name: &str,
    tool_text: Option<&str>,
) -> VerifyConclusion {
    if family == OperationFamily::AppAccess {
        return match tool_text.and_then(|t| {
            crate::agents::computer::tools::parse_app_access_host_outcome(root_name, t)
        }) {
            Some(crate::agents::computer::tools::AppAccessHostOutcome::Pass) => VerifyConclusion {
                action_result: ActionResult::Pass,
                failure_cause: None,
                step_summary: Some(
                    crate::agents::computer::tools::app_access_host_pass_summary(
                        root_name,
                        tool_text.unwrap_or_default(),
                    ),
                ),
            },
            Some(crate::agents::computer::tools::AppAccessHostOutcome::Fail) => VerifyConclusion {
                action_result: ActionResult::Fail,
                failure_cause: Some(FailureCause::WrongOperation),
                step_summary: tool_text
                    .and_then(|t| t.lines().next())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            },
            None => VerifyConclusion {
                action_result: ActionResult::Na,
                failure_cause: None,
                step_summary: None,
            },
        };
    }

    VerifyConclusion {
        action_result: ActionResult::Na,
        failure_cause: None,
        step_summary: None,
    }
}

fn clipboard_early_fail_conclusion(
    root_name: &str,
    tool_text: Option<&str>,
) -> Option<VerifyConclusion> {
    let (step_summary, kind) =
        match crate::agents::computer::tools::clipboard_host_verify_kind(root_name, tool_text) {
            crate::agents::computer::tools::ClipboardHostVerifyKind::Defer => return None,
            crate::agents::computer::tools::ClipboardHostVerifyKind::EmptyClipboard => (
                "Clipboard is empty after read; expected content.".to_string(),
                "empty",
            ),
            crate::agents::computer::tools::ClipboardHostVerifyKind::ZeroWritten => (
                "Clipboard write copied zero characters.".to_string(),
                "zero_written",
            ),
            crate::agents::computer::tools::ClipboardHostVerifyKind::ToolError => {
                let summary = tool_text
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.lines().next().unwrap_or(s).trim().to_string())
                    .unwrap_or_else(|| "Clipboard tool result missing or unreadable.".to_string());
                (summary, "tool_error")
            }
        };
    log::info!("computer pipeline: clipboard host early fail tool={root_name} reason={kind}");
    Some(VerifyConclusion {
        action_result: ActionResult::Fail,
        failure_cause: Some(FailureCause::WrongOperation),
        step_summary: Some(step_summary),
    })
}

async fn apply_host_verify_shortcut(
    state: &AppState,
    conversation_id: &str,
    root_name: &str,
    root_args: &Value,
    conclusion: &VerifyConclusion,
    log_label: &str,
) -> Result<()> {
    let (after_cap, _) = state
        .computer_state
        .capture_and_annotate(conversation_id)
        .await?;
    state
        .computer_state
        .store_pipeline_after_capture(conversation_id, after_cap);
    state
        .computer_state
        .apply_pipeline_verify_result(conversation_id, conclusion);
    state.computer_state.set_pipeline_last_operation(
        conversation_id,
        format_operation_summary(root_name, root_args),
    );
    log::info!(
        "computer pipeline: {log_label} conversation_id={conversation_id} tool={root_name} result={}",
        conclusion.action_result.as_history_str()
    );
    Ok(())
}

fn tool_execution_host_fail_conclusion(tool_text: Option<&str>) -> Option<VerifyConclusion> {
    let text = tool_text?.trim();
    if text.is_empty() {
        return None;
    }
    let failed = text.starts_with("ERROR:")
        || text.contains("FAILED —")
        || text.contains("failed:")
        || text.contains("No valid indices resolved");
    if !failed {
        return None;
    }
    log::info!("computer pipeline: tool execution error — host verify fail");
    Some(VerifyConclusion {
        action_result: ActionResult::Fail,
        failure_cause: Some(FailureCause::WrongOperation),
        step_summary: Some(
            text.lines()
                .next()
                .unwrap_or(text)
                .trim()
                .trim_start_matches("ERROR:")
                .trim()
                .to_string(),
        ),
    })
}

/// Post-execute verification for desktop tools when verify host is enabled.
pub async fn run_pipeline_post_execute_verify(
    state: &AppState,
    stream: &StreamTx,
    conversation_id: &str,
    provider: &OpenAIProvider,
    settings: &ModelSettings,
    before_cap: Option<&ScreenCaptureResult>,
    message_id: &str,
    history: &[ChatMessage],
    tool_calls: &[ToolCall],
    cancel: CancellationToken,
    usage: Option<&mut PipelineLlmUsageRecorder<'_>>,
) -> Result<()> {
    let Some(root_name) = find_root_tool_name(tool_calls, state.tools.as_ref()) else {
        return Ok(());
    };
    let root_tc = tool_calls
        .iter()
        .find(|tc| tc.name.trim() == root_name)
        .or_else(|| tool_calls.first());
    let root_args: Value = root_tc
        .and_then(|tc| serde_json::from_str(&tc.arguments).ok())
        .unwrap_or(Value::Null);
    let tool_text = root_tc
        .and_then(|tc| tool_result_text_for_call(history, message_id, &tc.id))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let family = operation_family_for_tool(&root_name);
    let tool_text = tool_text.as_deref();
    let round_seq = state
        .computer_state
        .current_pipeline_round_seq(conversation_id);
    let file_prefix = format!("{conversation_id}_{round_seq}");

    if family == OperationFamily::AppAccess {
        let conclusion = host_verify_conclusion(family, &root_name, tool_text);
        apply_host_verify_shortcut(
            state,
            conversation_id,
            &root_name,
            &root_args,
            &conclusion,
            "app access host verify applied",
        )
        .await?;
        return Ok(());
    }

    if family == OperationFamily::Clipboard {
        if let Some(conclusion) = clipboard_early_fail_conclusion(&root_name, tool_text) {
            apply_host_verify_shortcut(
                state,
                conversation_id,
                &root_name,
                &root_args,
                &conclusion,
                "clipboard host verify (skip LLM)",
            )
            .await?;
            return Ok(());
        }
    }

    if let Some(conclusion) = tool_execution_host_fail_conclusion(tool_text) {
        apply_host_verify_shortcut(
            state,
            conversation_id,
            &root_name,
            &root_args,
            &conclusion,
            "tool execution error host verify (skip LLM)",
        )
        .await?;
        return Ok(());
    }

    if needs_verify_for_tool(&root_name, &root_args) {
        let Some(before) = before_cap else {
            log::warn!(
                "computer pipeline: verify skipped — missing before capture conversation_id={conversation_id} tool={root_name}"
            );
            return apply_na_verify_fallback(state, conversation_id, &root_name, &root_args).await;
        };
        match run_verify_phase(
            state.computer_state.as_ref(),
            conversation_id,
            message_id,
            provider,
            settings,
            &root_name,
            &root_args,
            before,
            tool_text,
            cancel,
            Some(&format!("{file_prefix}_verify")),
            &file_prefix,
            "verify",
            Some(stream),
            usage,
        )
        .await
        {
            Ok(_) => {
                state.computer_state.set_pipeline_last_operation(
                    conversation_id,
                    format_operation_summary(&root_name, &root_args),
                );
            }
            Err(err) => {
                log::warn!(
                    "computer pipeline: verify phase failed conversation_id={conversation_id} tool={root_name}: {err:#}"
                );
                apply_na_verify_fallback(state, conversation_id, &root_name, &root_args).await?;
            }
        }
        return Ok(());
    }

    apply_na_verify_fallback(state, conversation_id, &root_name, &root_args).await
}

async fn apply_na_verify_fallback(
    state: &AppState,
    conversation_id: &str,
    root_name: &str,
    root_args: &Value,
) -> Result<()> {
    let conclusion = VerifyConclusion {
        action_result: ActionResult::Na,
        failure_cause: None,
        step_summary: None,
    };
    let (after_cap, _) = state
        .computer_state
        .capture_and_annotate(conversation_id)
        .await?;
    state
        .computer_state
        .store_pipeline_after_capture(conversation_id, after_cap);
    state
        .computer_state
        .apply_pipeline_verify_result(conversation_id, &conclusion);
    state.computer_state.set_pipeline_last_operation(
        conversation_id,
        format_operation_summary(root_name, root_args),
    );
    log::info!(
        "computer pipeline: verify n/a applied conversation_id={conversation_id} tool={root_name}"
    );
    Ok(())
}

fn loose_bool_arg(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => {
            matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes")
        }
        Some(Value::Number(n)) => n.as_i64().is_some_and(|i| i != 0),
        _ => false,
    }
}

fn arg_u32(v: Option<&Value>) -> Option<u32> {
    match v? {
        Value::Number(n) => n.as_u64().and_then(|u| u32::try_from(u).ok()),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn arg_str(v: Option<&Value>) -> Option<&str> {
    v.and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn execution_summary_for_tool_card(
    root_name: &str,
    args: &Value,
    tool_text: Option<&str>,
) -> Option<String> {
    if let Some(t) = tool_text.map(str::trim).filter(|s| !s.is_empty()) {
        let lower = t.to_lowercase();
        if lower.contains(" ok —") || lower.contains("verified:") || lower.contains("failed —")
        {
            return Some(t.lines().next().unwrap_or(t).trim().to_string());
        }
    }

    let name = root_name.trim().to_ascii_lowercase();
    let text = arg_str(args.get("text"));
    match name.as_str() {
        "input_index" => {
            let index = arg_u32(args.get("index"))?;
            let payload = text?;
            let mut line = format!("Executed input_index at overlay {index}: typed \"{payload}\"");
            if loose_bool_arg(args.get("clear_first")) {
                line.push_str("; cleared first");
            }
            if loose_bool_arg(args.get("auto_enter")) {
                line.push_str("; Enter sent");
            }
            Some(line)
        }
        "input_at" => {
            let x = arg_u32(args.get("x"))?;
            let y = arg_u32(args.get("y"))?;
            let payload = text?;
            let mut line = format!("Executed input_at at ({x}, {y}): typed \"{payload}\"");
            if loose_bool_arg(args.get("clear_first")) {
                line.push_str("; cleared first");
            }
            if loose_bool_arg(args.get("auto_enter")) {
                line.push_str("; Enter sent");
            }
            Some(line)
        }
        "input_focused" => {
            let payload = text?;
            let mut line = format!("Executed input_focused: typed \"{payload}\"");
            if loose_bool_arg(args.get("clear_first")) {
                line.push_str("; cleared first");
            }
            if loose_bool_arg(args.get("auto_enter")) {
                line.push_str("; Enter sent");
            }
            Some(line)
        }
        _ => {
            if let Some(t) = tool_text.map(str::trim).filter(|s| !s.is_empty()) {
                let first = t.lines().next().unwrap_or(t).trim();
                if !is_internal_tool_log_line(first)
                    && !first.to_ascii_lowercase().contains("do not assume")
                {
                    return Some(first.to_string());
                }
            }
            let action = arg_str(args.get("action"));
            let goal = arg_str(args.get("goal"));
            let index = arg_u32(args.get("index"));
            if name.starts_with("mouse_click")
                || name.starts_with("mouse_double_click")
                || name.starts_with("mouse_right_click")
            {
                let label = action.or(goal)?;
                return Some(match index {
                    Some(i) => format!("Executed {root_name} at overlay {i}: {label}"),
                    None => format!("Executed {root_name}: {label}"),
                });
            }
            match (goal, action) {
                (Some(g), _) => Some(format!("Executed {root_name}: {g}")),
                (_, Some(a)) => Some(format!("Executed {root_name}: {a}")),
                _ => None,
            }
        }
    }
}

fn build_tool_card_text_after_verify(
    root_name: &str,
    root_args: &Value,
    verify: &VerifyConclusion,
    family: OperationFamily,
    tool_text: Option<&str>,
) -> String {
    let verify_line = verify.verify_line_for_tool_card();
    if family == OperationFamily::AppAccess {
        if let Some(exec) = execution_summary_for_tool_card(root_name, root_args, tool_text) {
            return format!("{exec}\n{verify_line}");
        }
        return verify_line;
    }
    match execution_summary_for_tool_card(root_name, root_args, tool_text) {
        Some(exec) => format!("{exec}\n{verify_line}"),
        None => verify_line,
    }
}

pub fn apply_pipeline_verify_to_tool_card(
    stream: &StreamTx,
    history: &mut Vec<ChatMessage>,
    message_id: &str,
    tool_calls: &[ToolCall],
    tools: &ToolRegistry,
    conversation_id: &str,
    verify: &VerifyConclusion,
    persist_transcript: bool,
) {
    let Some(root_name) = find_root_tool_name(tool_calls, tools) else {
        return;
    };
    let root_args: Value = tool_calls
        .iter()
        .find(|tc| tc.name.trim() == root_name)
        .and_then(|tc| serde_json::from_str(&tc.arguments).ok())
        .unwrap_or(Value::Null);
    let family = operation_family_for_tool(&root_name);
    if family == OperationFamily::AppAccess && verify.action_result != ActionResult::Fail {
        return;
    }
    let root_tc = tool_calls
        .iter()
        .find(|tc| tc.name.trim() == root_name)
        .or_else(|| tool_calls.first());
    let Some(tc) = root_tc else {
        return;
    };

    let tool_text = tool_result_text_for_call(history, message_id, &tc.id);
    let card_text = build_tool_card_text_after_verify(
        &root_name,
        &root_args,
        verify,
        family,
        tool_text.as_deref(),
    );

    if family == OperationFamily::AppAccess {
        emit(
            stream,
            StreamEvent::ToolCallStatus {
                message_id: message_id.to_string(),
                tool_call_id: tc.id.clone(),
                status: "failed".to_string(),
                result: None,
                error: Some(card_text.clone()),
                duration_ms: None,
                display_label: None,
                display_summary: None,
                trace_id: None,
                scoped_message_id: None,
            },
        );
    } else {
        emit(
            stream,
            StreamEvent::ToolCallStatus {
                message_id: message_id.to_string(),
                tool_call_id: tc.id.clone(),
                status: "success".to_string(),
                result: Some(card_text.clone()),
                error: None,
                duration_ms: None,
                display_label: None,
                display_summary: None,
                trace_id: None,
                scoped_message_id: None,
            },
        );
    }

    if persist_transcript {
        crate::conversation_transcript::record_tool_result(
            conversation_id,
            history,
            message_id,
            &tc.id,
            &card_text,
        );
    } else {
        crate::conversation_transcript::insert_tool_result_in_history(
            history, message_id, &tc.id, &card_text,
        );
    }
    log::info!(
        "computer pipeline: tool card updated from host verify conversation_id={conversation_id} tool={root_name} result={}",
        verify.action_result.as_history_str()
    );
}

pub fn pipeline_give_up_error(state: &AppState, conversation_id: &str) -> Option<anyhow::Error> {
    if state.computer_state.should_give_up(conversation_id) {
        Some(anyhow!(
            "当前任务已尽力但仍无法完成（重复操作达到 {} 次），请提供进一步指导。",
            crate::agents::computer::tier::GIVE_UP_THRESHOLD
        ))
    } else {
        None
    }
}
