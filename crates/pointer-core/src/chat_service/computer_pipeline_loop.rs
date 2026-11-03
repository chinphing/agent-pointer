//! Shared host verify hooks for single-agent and sub-agent loops.

use crate::agents::computer::pipeline::operation::{
    format_operation_summary, needs_verify_for_tool, operation_family_for_tool, OperationFamily,
};
use crate::agents::computer::pipeline::types::{
    ActionResult, FailureCause, ScrollAxis, VerifyConclusion,
};
pub use crate::agents::computer::pipeline::PipelineLlmUsageRecorder;
use crate::agents::computer::pipeline::{
    batch_family, find_root_tool_name, run_verify_phase, VerifyTarget,
};
use crate::agents::computer::tools::args_util::SCROLL_NOTCHES_MAX;
use crate::agents::computer::ScreenCaptureResult;
use crate::agents::AgentProfile;
use crate::chat_service::app_state::AppState;
use crate::chat_service::emit::emit;
use crate::chat_service::StreamTx;
use crate::models::{ChatMessage, ModelSettings, Role, StreamEvent, ToolCall};
use crate::tools::ToolRegistry;
use anyhow::{anyhow, Result};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

pub fn verify_host_active(state: &AppState, profile: &AgentProfile) -> bool {
    *profile == AgentProfile::Computer && state.computer_state.verify_host_enabled()
}

pub fn batch_has_desktop_root_tool(tool_calls: &[ToolCall], tools: &ToolRegistry) -> bool {
    // Any desktop call that writes a verdict opens the gate — not just the first one.
    //
    // `needs_verify_for_tool` answers "does the verify LLM need to run", which is
    // false for `AppAccess`: `launch_app` / `list_apps` are judged from the tool
    // text by the host, no screenshot LLM involved. But they still write a
    // verdict row, so gating on `needs_verify_for_tool` alone left that host-only
    // path unreachable. `Other` covers `locate_element` and unknown tools, which
    // have nothing to verify at all.
    tool_calls.iter().any(|tc| {
        let name = tc.name.trim();
        if tools.get_def(name).is_none()
            || crate::task_board::checkpoint::is_task_board_tool_name(name)
        {
            return false;
        }
        matches!(
            operation_family_for_tool(name),
            OperationFamily::AppAccess
        ) || crate::agents::computer::pipeline::operation::needs_verify_for_tool(
            name,
            &Value::Null,
        )
    })
}

#[cfg(test)]
mod batch_gate_tests {
    use super::*;
    use crate::agents::computer::pipeline::operation::needs_verify_for_tool;

    // locate_element 只读、不改桌面状态，抓 before 截图没有意义。
    #[test]
    fn locate_does_not_need_verify() {
        assert!(!needs_verify_for_tool("locate_element", &Value::Null));
    }

    // clipboard 的成功/失败要看内容变化，仍然需要 verify。
    #[test]
    fn clipboard_needs_verify() {
        assert!(needs_verify_for_tool("clipboard_write", &Value::Null));
    }

    #[test]
    fn click_needs_verify() {
        assert!(needs_verify_for_tool("mouse_click_at", &Value::Null));
    }

    #[test]
    fn input_needs_verify() {
        assert!(needs_verify_for_tool("input_at", &Value::Null));
    }

    #[test]
    fn app_access_opens_the_gate_even_without_llm_verify() {
        // `launch_app` / `list_apps` are judged from tool text by the host, so
        // `needs_verify()` is false for them — but they still write a verdict row.
        // Gating the whole pipeline on `needs_verify()` alone made the AppAccess
        // host path unreachable.
        assert!(!needs_verify_for_tool("launch_app", &Value::Null));
        assert!(matches!(
            operation_family_for_tool("launch_app"),
            OperationFamily::AppAccess
        ));
    }

    #[test]
    fn batch_family_falls_back_to_generic_when_app_access_mixes_with_a_click() {
        // `[launch_app, mouse_click_at]` must not take the AppAccess host path —
        // that would judge the launch and never look at the click.
        let launch = VerifyTarget::new("launch_app", Value::Null, None);
        let click = VerifyTarget::new("mouse_click_at", Value::Null, None);
        assert_eq!(batch_family(&[launch.clone()]), OperationFamily::AppAccess);
        assert_eq!(batch_family(&[launch, click]), OperationFamily::Other);
    }
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

/// Every desktop tool in the batch, in execution order.
///
/// A turn may emit several desktop calls. They run serially and are judged by a
/// single verify pass over the before/after frames, so verify needs all of them —
/// judging only the first left the rest unverified while the conclusion still
/// landed on the newest history row.
pub fn collect_desktop_tools(
    calls: &[ToolCall],
    tools: &ToolRegistry,
    history: &[ChatMessage],
    message_id: &str,
) -> Vec<VerifyTarget> {
    let mut out = Vec::new();
    for tc in calls {
        let name = tc.name.trim();
        if crate::task_board::checkpoint::is_task_board_tool_name(name)
            || tools.get_def(name).is_none()
        {
            continue;
        }
        if !crate::agents::computer::input::timing::is_desktop_vision_log_tool(name) {
            continue;
        }
        let args = serde_json::from_str(&tc.arguments).unwrap_or(Value::Null);
        let tool_result = tool_result_text_for_call(history, message_id, &tc.id)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        out.push(VerifyTarget::new(name, args, tool_result));
    }
    out
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
                scroll: None,
            },
            Some(crate::agents::computer::tools::AppAccessHostOutcome::Fail) => VerifyConclusion {
                action_result: ActionResult::Fail,
                failure_cause: Some(FailureCause::WrongOperation),
                step_summary: tool_text
                    .and_then(|t| t.lines().next())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
                scroll: None,
            },
            None => VerifyConclusion {
                action_result: ActionResult::Na,
                failure_cause: None,
                step_summary: None,
                scroll: None,
            },
        };
    }

    VerifyConclusion {
        action_result: ActionResult::Na,
        failure_cause: None,
        step_summary: None,
        scroll: None,
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
        scroll: None,
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
        .capture_screen(conversation_id)
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

/// Did the tool fail because it could not locate its target?
///
/// Grounding failure is not an operation failure: nothing was clicked, so the
/// screen did not change and there is nothing to verify. Treating it as a failed
/// operation wrote `Verify: fail (wrong_operation)` with the locate error copied
/// into it, which showed the same sentence twice on one card and claimed an
/// operation had gone wrong when none had run.
///
/// Read from the payload, not from text: `{"success": false, …}` is the same
/// shape `terminal` returns, so this does not depend on the wording of a message.
fn is_grounding_failure(tool_text: Option<&str>) -> bool {
    let Some(text) = tool_text.map(str::trim) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| {
            let failed = v.get("success").and_then(|x| x.as_bool()) == Some(false);
            let has_status = v.get("status").and_then(|x| x.as_str()).is_some();
            (failed && has_status).then_some(())
        })
        .is_some()
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
        scroll: None,
    })
}

/// Post-execute verification for desktop tools when verify host is enabled.
pub async fn run_pipeline_post_execute_verify(
    state: &AppState,
    stream: &StreamTx,
    conversation_id: &str,
    settings: &ModelSettings,
    before_cap: Option<&ScreenCaptureResult>,
    message_id: &str,
    history: &[ChatMessage],
    tool_calls: &[ToolCall],
    cancel: CancellationToken,
    usage: Option<&mut PipelineLlmUsageRecorder<'_>>,
) -> Result<()> {
    let targets = collect_desktop_tools(tool_calls, state.tools.as_ref(), history, message_id);
    let Some(root_name) = targets.first().map(|t| t.name.clone()) else {
        return Ok(());
    };
    let root_args = targets
        .first()
        .map(|t| t.args.clone())
        .unwrap_or(Value::Null);
    let tool_text = targets
        .first()
        .and_then(|t| t.tool_result.clone())
        .or_else(|| {
            // Fallback: the first desktop call may have no recorded result while a
            // later one does. Any non-empty result still tells us whether the batch
            // errored out.
            targets.iter().find_map(|t| t.tool_result.clone())
        });
    // Judge the batch, not the first call: `[launch_app, mouse_click_at]` used to
    // take the AppAccess host path and never look at the click.
    let family = batch_family(&targets);
    let tool_text = tool_text.as_deref();
    let round_seq = state
        .computer_state
        .current_pipeline_round_seq(conversation_id);
    let file_prefix = format!("{conversation_id}_{round_seq}");

    if family == OperationFamily::AppAccess {
        let conclusion = host_verify_conclusion(family, &root_name, tool_text);
        // No parseable outcome -> nothing worth showing on the card. Writing
        // `n/a` here put a meaningless "Verify: n/a" line under launch_app /
        // list_apps rows.
        if conclusion.action_result == ActionResult::Na {
            return Ok(());
        }
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

    if is_grounding_failure(tool_text) {
        log::info!(
            "computer pipeline: grounding failed — nothing was clicked, skipping verify"
        );
        // Nothing was clicked, so nothing can be judged — but leaving the row
        // open renders `verify: verifying`, and the prompt forbids new desktop
        // tools in that state. Close it so the model can retry the step.
        state.computer_state.apply_pipeline_skipped_verify(
            conversation_id,
            "grounding failed; nothing was clicked",
        );
        return Ok(());
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

    // Tools that cannot change the desktop (locate_element, clipboard, app
    // access) have nothing to verify. Writing an `n/a` result for them put a
    // meaningless "Verify: n/a" line on every locate card.
    //
    // Gate on the batch, not on the first call: a `key_press` ahead of a click
    // classifies as `Other`, and judging by that alone would drop the click.
    let any_needs_verify = targets
        .iter()
        .any(|t| needs_verify_for_tool(&t.name, &t.args));
    if !any_needs_verify {
        return Ok(());
    }

    {
        let Some(before) = before_cap else {
            log::warn!(
                "computer pipeline: verify skipped — missing before capture conversation_id={conversation_id} tool={root_name}"
            );
            // No "before" frame means we cannot judge — that is `pending`, not
            // "no verification needed".
            let err = anyhow::anyhow!("missing before capture");
            return apply_failed_verify_fallback(
                state,
                conversation_id,
                &root_name,
                &root_args,
                &err,
            )
            .await;
        };
        match run_verify_phase(
            state.computer_state.as_ref(),
            conversation_id,
            message_id,
            settings,
            &targets,
            before,
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
                // A failed verify is NOT "no verify needed". Reporting `n/a` here
                // makes the main LLM treat the action as verified and move on —
                // which is how a broken API key silently turns into wrong
                // coordinates surviving to the next step. `pending` keeps the
                // row open so the model knows the outcome is unknown.
                apply_failed_verify_fallback(state, conversation_id, &root_name, &root_args, &err)
                    .await?;
            }
        }
    }
    Ok(())
}

/// Verify ran but the LLM call itself failed (bad key, timeout, 5xx).
///
/// Records `skipped`, not `pending`. `pending` was rejected by both backfill
/// functions in `runtime/mod.rs`, so it left the row rendering
/// `verify: verifying` forever — and the prompt forbids new desktop tools in
/// that state, which locks the agent out of retrying. `skipped` closes the row
/// and says what actually happened: nobody judged this one.
async fn apply_failed_verify_fallback(
    state: &AppState,
    conversation_id: &str,
    root_name: &str,
    root_args: &Value,
    err: &anyhow::Error,
) -> Result<()> {
    let (after_cap, _) = state
        .computer_state
        .capture_screen(conversation_id)
        .await?;
    state
        .computer_state
        .store_pipeline_after_capture(conversation_id, after_cap);
    let closed = state.computer_state.apply_pipeline_skipped_verify(
        conversation_id,
        &format!("verify unavailable: {err}"),
    );
    state.computer_state.set_pipeline_last_operation(
        conversation_id,
        format_operation_summary(root_name, root_args),
    );
    log::warn!(
        "computer pipeline: verify skipped (LLM call failed) conversation_id={conversation_id} tool={root_name} closed_rows={closed}"
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

/// 从 `success_payload` 里取回**校准后**的落点。
///
/// `point` 是 `resolve_point` 返回的 locate 精定位中心，已经过
/// `report_coordinate` 反算成模型输入侧的那套坐标（多屏时不带显示器偏移）。
/// verify 覆盖 tool 消息时如果把它丢掉，模型就再也拿不回「我到底点了哪里」。
fn calibrated_point(tool_text: Option<&str>) -> Option<(i64, i64)> {
    let v: serde_json::Value = serde_json::from_str(tool_text?.trim()).ok()?;
    let p = v.get("point")?.as_array()?;
    let x = p.first()?.as_f64()?;
    let y = p.get(1)?.as_f64()?;
    Some((x.round() as i64, y.round() as i64))
}

/// locate 校准后的目标框，`[x1, y1, x2, y2]`。
///
/// 和 `point` 成对出现，但 `point` 只说落点、不说它命中的是什么。带框回来，
/// 模型才能在下一次操作里用「就在刚才那个框附近」去描述新目标，而不是重新
/// 从整屏猜一遍。这是 `success_payload` 里本来就有的 `box`，不是模型传的
/// `bbox`——后者只是它的输入假设，这里不参与。
/// 多目标时的落点与框：`points` / `boxes` 复数形态。
///
/// 批量点选（勾选多个复选框）走这条路径。只读单数 `point` 会让整条结果
/// 只剩「点了 N 下」，一个坐标都不剩。
fn calibrated_points(tool_text: Option<&str>) -> Option<Vec<((i64, i64), Option<[i64; 4]>)>> {
    let v: serde_json::Value = serde_json::from_str(tool_text?.trim()).ok()?;
    let pts = v.get("points")?.as_array()?;
    if pts.len() < 2 {
        return None;
    }
    let boxes = v.get("boxes").and_then(|b| b.as_array());
    let mut out = Vec::with_capacity(pts.len());
    for (i, p) in pts.iter().enumerate() {
        let a = p.as_array()?;
        let x = a.first()?.as_f64()?.round() as i64;
        let y = a.get(1)?.as_f64()?.round() as i64;
        let b = boxes
            .and_then(|bs| bs.get(i))
            .and_then(|b| b.as_array())
            .filter(|b| b.len() == 4)
            .and_then(|b| {
                let mut o = [0i64; 4];
                for (k, n) in b.iter().enumerate() {
                    o[k] = n.as_f64()?.round() as i64;
                }
                Some(o)
            });
        out.push(((x, y), b));
    }
    Some(out)
}

/// `(605, 307) box [588, 291, 612, 309]`，框缺失时只留落点。
fn fmt_point_box(pb: &((i64, i64), Option<[i64; 4]>)) -> String {
    let ((x, y), b) = pb;
    match b {
        Some([x1, y1, x2, y2]) => format!("({x}, {y}) box [{x1}, {y1}, {x2}, {y2}]"),
        None => format!("({x}, {y})"),
    }
}

fn calibrated_box(tool_text: Option<&str>) -> Option<[i64; 4]> {
    let v: serde_json::Value = serde_json::from_str(tool_text?.trim()).ok()?;
    let b = v.get("box")?.as_array()?;
    if b.len() != 4 {
        return None;
    }
    let mut out = [0i64; 4];
    for (i, n) in b.iter().enumerate() {
        out[i] = n.as_f64()?.round() as i64;
    }
    Some(out)
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
        "input_at" => {
            let payload = text?;
            // 落点和框都从 payload 取：`args` 里的 x/y 是 dispatch 回写的同一份
            // 数据，但框只在 payload 里。缺 `point` 时退回 args 的 x/y，不编框。
            let calibrated = calibrated_point(tool_text).or_else(|| {
                Some((
                    arg_u32(args.get("x"))? as i64,
                    arg_u32(args.get("y"))? as i64,
                ))
            });
            let mut line = match calibrated {
                Some((x, y)) => format!("Executed input_at — typed into calibrated point ({x}, {y})"),
                None => "Executed input_at".to_string(),
            };
            if let Some([x1, y1, x2, y2]) = calibrated_box(tool_text) {
                line.push_str(&format!("; target box [{x1}, {y1}, {x2}, {y2}]"));
            }
            line.push_str(&format!(": typed \"{payload}\""));
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
        // 带坐标的工具**先于**通用文本回退处理。
        //
        // 通用回退靠「文本里有没有 do not assume」判断要不要原样返回 payload，
        // 那是 `note` 的措辞 —— 一改措辞，坐标就悄悄丢了。落点是 verify
        // 判 fail 后模型唯一能用来区分「差一点」和「找错元素」的东西，不能
        // 依赖别处的文案。
        "mouse_click_at" | "mouse_double_click_at" | "mouse_right_click_at" | "move_to"
        | "mouse_drag_from_to_at" | "mouse_scroll_current"
        | "modified_click_select_at" | "modified_click_range_select_at" => {
            // 落点必须是**校准后**的那个，不是模型给的 `bbox` 中心。
            //
            // 模型给 `bbox` 只是「大概在这」，locate 会在裁片上精定位，偏差
            // 通常 5–20 px；`report_coordinate` 再把它反算成截图内像素。报
            // bbox 中心等于报模型自己的猜测，verify 说 fail 时它会以为「位置
            // 是对的、操作没生效」，而不是「我一开始就找错地方」。
            let verb = if name.contains("drag") {
                "dragged"
            } else if name.contains("scroll") {
                "scrolled"
            } else if name == "move_to" {
                "moved to"
            } else {
                "clicked"
            };
            // 多目标先判：`success_payload` 在 `points.len() > 1` 时只写复数
            // 字段，单数读取会拿到 None，坐标就全丢了。
            if let Some(multi) = calibrated_points(tool_text) {
                let head = arg_str(args.get("target_desc"))
                    .or_else(|| arg_str(args.get("action")))
                    .or_else(|| arg_str(args.get("goal")));
                // 两点的语义随工具变：drag 是「抓起点 / 放终点」同一对动作的
                // 两端，不是两个独立目标。写成 `#0 / #1` 会让模型以为点了两下。
                let line = if name.contains("drag") && multi.len() == 2 {
                    let (a, b) = (&multi[0], &multi[1]);
                    let mut l = match head {
                        Some(h) => format!("Executed {root_name}: {h} — dragged"),
                        None => format!("Executed {root_name} — dragged"),
                    };
                    l.push_str(&format!(" from calibrated point {}", fmt_point_box(a)));
                    l.push_str(&format!(" to calibrated point {}", fmt_point_box(b)));
                    l
                } else {
                    let mut l = match head {
                        Some(h) => format!(
                            "Executed {root_name}: {h} — {verb} {} calibrated points",
                            multi.len()
                        ),
                        None => format!(
                            "Executed {root_name} — {verb} {} calibrated points",
                            multi.len()
                        ),
                    };
                    for (n, pb) in multi.iter().enumerate() {
                        l.push_str(&format!("; #{n}: {}", fmt_point_box(pb)));
                    }
                    l
                };
                return Some(line);
            }
            let calibrated = calibrated_point(tool_text);
            let boxed = calibrated_box(tool_text);
            // `target_desc` 是模型描述目标的主字段，`action`/`goal` 是旧别名。
            // 只读后者会让最常见的那种调用在摘要里丢掉「点的是什么」。
            let label = arg_str(args.get("target_desc"))
                .or_else(|| arg_str(args.get("action")))
                .or_else(|| arg_str(args.get("goal")));
            let index = arg_u32(args.get("index"));
            let mut line = match (label, calibrated, index) {
                (Some(l), Some((x, y)), _) => {
                    format!("Executed {root_name}: {l} — {verb} calibrated point ({x}, {y})")
                }
                (None, Some((x, y)), _) => {
                    format!("Executed {root_name} — {verb} calibrated point ({x}, {y})")
                }
                (Some(l), None, Some(i)) => format!("Executed {root_name} at overlay {i}: {l}"),
                (Some(l), None, None) => format!("Executed {root_name}: {l}"),
                (None, None, Some(i)) => format!("Executed {root_name} at overlay {i}"),
                (None, None, None) => format!("Executed {root_name}"),
            };
            // 「calibrated」只对落点成立时才写，别让它变成一句没有依据的形容词。
            if calibrated.is_none() {
                line = line.replace(" calibrated", "");
            }
            if let Some([x1, y1, x2, y2]) = boxed {
                line.push_str(&format!("; target box [{x1}, {y1}, {x2}, {y2}]"));
            }
            // 滚动量只在参数里，payload 没有。不报的话「滚了多少」全丢，
            // 下一次该滚几格只能重新猜。
            if name.contains("scroll") {
                let nx = args.get("notches_x").and_then(|v| v.as_f64());
                let ny = args.get("notches_y").and_then(|v| v.as_f64());
                match (nx, ny) {
                    (Some(x), Some(y)) if x != 0.0 || y != 0.0 => {
                        line.push_str(&format!("; scrolled {x} x-notches, {y} y-notches"))
                    }
                    (Some(y), None) if y != 0.0 => line.push_str(&format!("; scrolled {y} y-notches")),
                    (None, Some(y)) if y != 0.0 => line.push_str(&format!("; scrolled {y} y-notches")),
                    _ => {}
                }
            }
            return Some(line);
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
            match (goal, action) {
                (Some(g), _) => Some(format!("Executed {root_name}: {g}")),
                (_, Some(a)) => Some(format!("Executed {root_name}: {a}")),
                _ => None,
            }
        }
    }
}

/// What one scroll should move, as a fraction of the target area.
///
/// Big enough to make progress and small enough to stop on the row you want.
/// The model only has to send the notch count this constant implies.
pub const SCROLL_TARGET_FRACTION: f32 = 0.6;

/// A move inside `[SCROLL_BAND_MIN, SCROLL_BAND_MAX]` is already the target
/// size — recommending a new count there would be noise, so the host stays
/// quiet and the model keeps the count it used.
///
/// `SCROLL_BAND_MIN` is the target itself: anything that moved less than the
/// target gets a bigger count. A separate, lower floor would let a scroll that
/// moved half of what was asked for pass without a word.
pub const SCROLL_BAND_MIN: f32 = 0.6;
pub const SCROLL_BAND_MAX: f32 = 0.8;

/// Host-side scroll scaling line appended to a scroll's tool result.
///
/// The Verify LLM answers 「content moved how far」; the arithmetic that turns
/// that into a notch count is deterministic and lives here, so the model never
/// has to (and never gets to) guess. Three bands:
///
/// - **under** `SCROLL_BAND_MIN` → scale up to `SCROLL_TARGET_FRACTION`
/// - **inside** the band → no recommendation, keep the count
/// - **over** `SCROLL_BAND_MAX` → halve the count (a move above 1.0 means whole
///   rows were never visible, so the result also says to scroll back)
///
/// Returns `None` when there is nothing to scale from — non-scroll, missing
/// measurement, or no notches on the moved axis.
pub fn scroll_scale_line(root_args: &Value, verify: &VerifyConclusion) -> Option<String> {
    // At the edge the answer is 「stop scrolling」, not a bigger number — the
    // `Verify: fail (at_edge)` line already says so, and suggesting a count
    // here would fight it.
    if verify.failure_cause == Some(FailureCause::AtEdge) {
        return None;
    }
    let m = verify.scroll?;
    // Clamp to what the tool actually delivered. `root_args` is the model's raw
    // request; the tool caps it at ±`SCROLL_NOTCHES_MAX` before scrolling, so a
    // raw `-1500` moved the distance of `-1000`. Scaling from the raw number
    // makes every later recommendation wrong by the same factor.
    let cap = i64::from(SCROLL_NOTCHES_MAX);
    let (used, axis_label, axis_key) = match m.axis {
        ScrollAxis::Vertical => (
            root_args
                .get("notches_y")
                .and_then(|v| v.as_i64())?
                .clamp(-cap, cap),
            "target-heights",
            "notches_y",
        ),
        ScrollAxis::Horizontal => (
            root_args
                .get("notches_x")
                .and_then(|v| v.as_i64())?
                .clamp(-cap, cap),
            "target-widths",
            "notches_x",
        ),
    };
    if used == 0 {
        return None;
    }
    let used_abs = used.unsigned_abs() as f32;
    let sign = if used < 0 { -1 } else { 1 };
    let moved = m.fraction;
    if moved <= 0.0 {
        // Nothing moved: scaling is meaningless — the wheel never reached a
        // scrollable area. Say that instead of inventing a bigger number.
        return Some(format!(
            "Scroll scale: {used} notches moved 0 {axis_label} — the wheel did not \
             reach a scrollable area. Pass target_desc + bbox so the pointer moves \
             into the panel first, or check the app."
        ));
    }

    if (SCROLL_BAND_MIN..=SCROLL_BAND_MAX).contains(&moved) {
        // Already the size we want. Any number here would push the model to
        // change a count that is working.
        return Some(format!(
            "Scroll scale: {used} notches moved ~{moved:.2} {axis_label} — that is the \
             target size ({SCROLL_TARGET_FRACTION} {axis_label}); keep this count."
        ));
    }

    if moved > SCROLL_BAND_MAX {
        let halved = ((used_abs / 2.0).round().max(1.0) as i64).min(cap);
        let mut line = format!(
            "Scroll scale: {used} notches moved ~{moved:.2} {axis_label} — over the target \
             {SCROLL_TARGET_FRACTION} {axis_label}. Next scroll: halve it → send \
             {axis_key}={}.",
            sign * halved
        );
        if moved > 1.0 {
            line.push_str(&format!(
                " It jumped more than a full panel: rows between the two frames were never \
                 visible — scroll back ~{SCROLL_TARGET_FRACTION} to read them before \
                 continuing."
            ));
        }
        return Some(line);
    }

    let per_notch = moved / used_abs;
    let needed = (SCROLL_TARGET_FRACTION / per_notch).round().max(1.0) as i64;
    let capped = needed.min(i64::from(SCROLL_NOTCHES_MAX));
    let full_panel = (used_abs / moved).round() as i64;
    let mut line = format!(
        "Scroll scale: {used} notches moved ~{moved:.2} {axis_label} (≈{full_panel} notches per \
         full panel). Next scroll for ~{SCROLL_TARGET_FRACTION} {axis_label}: send \
         {axis_key}={}",
        sign * capped
    );
    if needed > capped {
        let calls = (needed + i64::from(SCROLL_NOTCHES_MAX) - 1) / i64::from(SCROLL_NOTCHES_MAX);
        line.push_str(&format!(
            " (wanted {needed}, capped at {SCROLL_NOTCHES_MAX}/call — ≈{calls} calls for that)"
        ));
    }
    line.push('.');
    Some(line)
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
    let base = match execution_summary_for_tool_card(root_name, root_args, tool_text) {
        Some(exec) => format!("{exec}\n{verify_line}"),
        None => verify_line,
    };
    if family != OperationFamily::Scroll {
        return base;
    }
    match scroll_scale_line(root_args, verify) {
        Some(scale) => format!("{base}\n{scale}"),
        None => base,
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
            crate::agents::computer::runtime::GIVE_UP_THRESHOLD
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        calibrated_point, execution_summary_for_tool_card, is_grounding_failure, scroll_scale_line,
        SCROLL_TARGET_FRACTION,
    };
    use crate::agents::computer::pipeline::types::{
        ActionResult, ScrollAxis, ScrollMeasurement, VerifyConclusion,
    };
    use serde_json::json;

    fn scroll_verify(fraction: f32, axis: ScrollAxis) -> VerifyConclusion {
        VerifyConclusion {
            action_result: ActionResult::Pass,
            failure_cause: None,
            step_summary: Some("moved".into()),
            scroll: Some(ScrollMeasurement { fraction, axis }),
        }
    }

    /// 50 notches → 0.05 target-heights：0.6 需要 600 notches，上限 1000 之内，
    /// 行里必须给出**能直接照抄**的 600。
    #[test]
    fn scroll_scale_gives_the_measured_count() {
        let args = json!({"notches_y": -50});
        let v = scroll_verify(0.05, ScrollAxis::Vertical);
        let line = scroll_scale_line(&args, &v).expect("有测量就该出缩放行");
        assert!(line.contains("notches_y=-600"), "没给出可照抄的值: {line}");
        assert!(!line.contains("capped"), "600 在上限内不该提截断: {line}");
        assert!(line.contains("≈1000 notches per full panel"), "没给全屏换算: {line}");
        assert_eq!(SCROLL_TARGET_FRACTION, 0.6);
    }

    /// 需求刻度超过 ±1000 时才截断，并报还要几次调用。
    #[test]
    fn scroll_scale_caps_at_the_per_call_limit() {
        let args = json!({"notches_y": -50});
        let v = scroll_verify(0.0004, ScrollAxis::Vertical);
        let line = scroll_scale_line(&args, &v).expect("行");
        assert!(line.contains("notches_y=-1000"), "{line}");
        assert!(line.contains("capped at 1000"), "没说明被截断: {line}");
        assert!(line.contains("calls"), "没给调用次数: {line}");
    }

    /// 需要的刻度小于上限时直接给准确值，不要报截断。
    #[test]
    fn scroll_scale_gives_the_exact_count_when_under_the_cap() {
        let args = json!({"notches_y": -10});
        let v = scroll_verify(0.25, ScrollAxis::Vertical);
        let line = scroll_scale_line(&args, &v).expect("缩放行");
        assert!(line.contains("notches_y=-24"), "10 刻度走 0.25，0.6 需要 24: {line}");
        assert!(!line.contains("capped"), "没到上限不该提截断: {line}");
    }

    /// 0.6–0.8 已经是目标尺寸：不给新刻度，明确让模型保持原值。
    /// 0.6 是下限本身 —— 低于它就一定给建议，不存在「勉强算达标」的区间。
    #[test]
    fn scroll_scale_stays_quiet_inside_the_target_band() {
        for moved in [0.6_f32, 0.7, 0.8] {
            let args = json!({"notches_y": -300});
            let v = scroll_verify(moved, ScrollAxis::Vertical);
            let line = scroll_scale_line(&args, &v).expect("行");
            assert!(line.contains("keep this count"), "{moved}: {line}");
            assert!(!line.contains("send notches"), "{moved} 在带内不该推荐刻度: {line}");
        }
    }

    /// 超过 0.8：相对上次刻度减半；超过 1（整屏以上）还要提示漏内容。
    #[test]
    fn scroll_scale_halves_after_an_overshoot() {
        let args = json!({"notches_y": -400});
        let v = scroll_verify(0.9, ScrollAxis::Vertical);
        let line = scroll_scale_line(&args, &v).expect("行");
        assert!(line.contains("notches_y=-200"), "400 减半是 200: {line}");
        assert!(!line.contains("never visible"), "0.9 未跨整屏，不该说漏内容: {line}");

        let v = scroll_verify(1.5, ScrollAxis::Vertical);
        let line = scroll_scale_line(&args, &v).expect("行");
        assert!(line.contains("notches_y=-200"), "400 减半是 200: {line}");
        assert!(line.contains("never visible"), "跨整屏必须提示漏内容: {line}");
        assert!(line.contains("scroll back"), "还要让模型回滚核对: {line}");
    }

    /// 方向必须跟随上一次滚动。
    #[test]
    fn scroll_scale_keeps_the_direction() {
        let v = scroll_verify(0.1, ScrollAxis::Vertical);
        let down = scroll_scale_line(&json!({"notches_y": 20}), &v).expect("行");
        assert!(down.contains("notches_y=120"), "{down}");
        let up = scroll_scale_line(&json!({"notches_y": -20}), &v).expect("行");
        assert!(up.contains("notches_y=-120"), "{up}");
    }

    /// 横向滚动按 width 轴算，并用 notches_x。
    #[test]
    fn scroll_scale_uses_the_moved_axis() {
        let args = json!({"notches_x": 30});
        let v = scroll_verify(0.1, ScrollAxis::Horizontal);
        let line = scroll_scale_line(&args, &v).expect("行");
        assert!(line.contains("notches_x=180"), "{line}");
        assert!(line.contains("target-widths"), "{line}");
    }

    /// 完全没动：不要凭空给一个更大的数字，要指向「指针没落在可滚区域」。
    #[test]
    fn scroll_scale_zero_move_points_at_the_pointer_not_a_bigger_count() {
        let args = json!({"notches_y": -50});
        let v = scroll_verify(0.0, ScrollAxis::Vertical);
        let line = scroll_scale_line(&args, &v).expect("行");
        assert!(line.contains("moved 0"), "{line}");
        assert!(line.contains("target_desc"), "没指向定位修法: {line}");
        assert!(!line.contains("notches_y="), "没动就不该建议刻度: {line}");
    }

    /// 已经滚到头：不给刻度建议，交给 `Verify: fail (at_edge)` 说「别滚了」。
    #[test]
    fn scroll_scale_says_nothing_at_the_edge() {
        let args = json!({"notches_y": -50});
        let mut v = scroll_verify(0.1, ScrollAxis::Vertical);
        v.action_result = ActionResult::Fail;
        v.failure_cause = Some(crate::agents::computer::pipeline::types::FailureCause::AtEdge);
        assert!(
            scroll_scale_line(&args, &v).is_none(),
            "at_edge 时不该再建议刻度"
        );
    }

    /// 没有测量 / 轴上没有刻度 / 非滚动：不出行，绝不猜。
    #[test]
    fn scroll_scale_stays_silent_without_measurement() {        let mut v = scroll_verify(0.1, ScrollAxis::Vertical);
        v.scroll = None;
        assert!(scroll_scale_line(&json!({"notches_y": -30}), &v).is_none());

        let v = scroll_verify(0.1, ScrollAxis::Vertical);
        assert!(scroll_scale_line(&json!({"notches_x": -30}), &v).is_none());
        assert!(scroll_scale_line(&json!({"notches_y": 0}), &v).is_none());
    }

    /// verify 覆盖 tool 消息后，模型仍要能拿回**校准后**的落点。
    ///
    /// `success_payload` 的 `point` 是 locate 精定位 + `report_coordinate`
    /// 反算的结果；摘要里报 `bbox` 中心等于报模型自己的猜测，verify 说 fail
    /// 时它会以为「位置对了、操作没生效」而不是「我一开始就找错地方」。
    #[test]
    fn calibrated_point_is_read_from_the_tool_payload() {
        let payload = r#"{"success":true,"action":"mouse_click_at","note":"...","point":[600,300],"box":[588,291,612,309]}"#;
        assert_eq!(calibrated_point(Some(payload)), Some((600, 300)));

        // 不是 JSON（比如 `hover` 的纯文本），不能编一个坐标出来。
        assert_eq!(calibrated_point(Some("Hovered at (600, 300).")), None);
        assert_eq!(calibrated_point(None), None);

        // 缺 `point` 也不能回退到 `box` 中心 —— 那是模型的猜测。
        let box_only = r#"{"success":true,"box":[588,291,612,309]}"#;
        assert_eq!(calibrated_point(Some(box_only)), None);
    }

    /// 摘要里必须出现校准坐标**和校准框**。
    #[test]
    fn mouse_summary_carries_the_calibrated_point_and_box() {
        let args = serde_json::json!({"target_desc": "保存按钮", "bbox": [580, 280, 620, 320]});
        let payload =
            r#"{"success":true,"action":"mouse_click_at","point":[605,307],"box":[588,291,612,309]}"#;
        let line = execution_summary_for_tool_card("mouse_click_at", &args, Some(payload))
            .expect("有 payload 就该出摘要");
        assert!(line.contains("(605, 307)"), "缺校准坐标: {line}");
        assert!(line.contains("calibrated"), "没标明是校准后的: {line}");
        assert!(
            line.contains("[588, 291, 612, 309]"),
            "缺校准框: {line}"
        );
        // bbox 中心是 (600, 300)，和校准点不同 —— 报错位置一眼能看出来。
        assert!(!line.contains("(600, 300)"), "报的是 bbox 中心不是校准点: {line}");
    }

    /// `input_at` 也要带校准点和框。
    #[test]
    fn input_at_summary_carries_the_calibrated_point_and_box() {
        let payload = r#"{"success":true,"action":"type","point":[605,307],"box":[588,291,612,309]}"#;
        let args = serde_json::json!({"text": "hello", "clear_first": true});
        let line = execution_summary_for_tool_card("input_at", &args, Some(payload))
            .expect("有 payload 就该出摘要");
        assert!(line.contains("(605, 307)"), "缺校准坐标: {line}");
        assert!(line.contains("[588, 291, 612, 309]"), "缺框: {line}");
        assert!(line.contains("typed \"hello\""), "缺输入内容: {line}");
        assert!(line.contains("cleared first"), "{line}");
    }

    /// `input_index` 已不存在，不该再有摘要分支。
    #[test]
    fn input_index_is_not_a_summary_branch() {
        let line = execution_summary_for_tool_card(
            "input_index",
            &serde_json::json!({"index": 12, "text": "hello"}),
            Some(r#"{"success":true,"index":12}"#),
        );
        if let Some(line) = line {
            assert!(!line.contains("overlay"), "input_index 还在出摘要: {line}");
        }
    }

    /// 滚动量在参数里，payload 没有 —— 不报就丢了。
    #[test]
    fn scroll_summary_carries_notches() {
        let payload = r#"{"success":true,"action":"mouse_scroll_current","note":"Scrolled."}"#;
        let args = serde_json::json!({"notches_x": 0, "notches_y": -3});
        let line = execution_summary_for_tool_card("mouse_scroll_current", &args, Some(payload))
            .expect("有 payload 就该出摘要");
        assert!(line.contains("-3 y-notches"), "缺滚动量: {line}");
    }

    /// drag 的两点读作「起点 → 终点」，不是两个独立目标。
    ///
    /// `mouse_drag_from_to_at` 分别 ground `from_*` 和 `to_*` 两处，所以 payload
    /// 里是复数。但它是一次拖拽的两端；写成 `#0 / #1` 会让模型以为点了两下。
    #[test]
    fn drag_summary_reads_two_points_as_from_and_to() {
        let payload = r#"{"success":true,"action":"mouse_drag_from_to_at","points":[[100,200],[300,400]],"boxes":[[90,190,110,210],[290,390,310,410]]}"#;
        let line = execution_summary_for_tool_card(
            "mouse_drag_from_to_at",
            &serde_json::json!({"to_target_desc": "回收站"}),
            Some(payload),
        )
        .expect("有 payload 就该出摘要");
        assert!(line.contains("dragged from calibrated point (100, 200)"), "{line}");
        assert!(line.contains("to calibrated point (300, 400)"), "{line}");
        assert!(line.contains("[90, 190, 110, 210]") && line.contains("[290, 390, 310, 410]"), "{line}");
        assert!(!line.contains("#0"), "drag 不该用编号列表: {line}");
    }

    /// 真正的多目标（`modified_click` 的 `positions[]`）才用编号列表。
    #[test]
    fn modified_click_summary_numbers_each_independent_target() {
        let payload = r#"{"success":true,"action":"modified_click_select_at","points":[[100,200],[300,400],[500,600]],"boxes":[[90,190,110,210],[290,390,310,410],[490,590,510,610]]}"#;
        let line = execution_summary_for_tool_card(
            "modified_click_select_at",
            &serde_json::json!({"goal": "勾选三项"}),
            Some(payload),
        )
        .expect("有 payload 就该出摘要");
        assert!(line.contains("3 calibrated points"), "{line}");
        assert!(line.contains("#0: (100, 200)") && line.contains("#2: (500, 600)"), "{line}");
    }

    /// 多目标（`points` / `boxes` 复数）也要带坐标和框回来。
    ///
    /// `success_payload` 在 `points.len() > 1` 时**只**写复数字段；只读单数
    /// `point` 会拿到 None，整条摘要就退化成「点了 N 下」，一个坐标都不剩。
    #[test]
    fn multi_target_summary_carries_every_point_and_box() {
        let payload = r#"{"success":true,"action":"mouse_click_at","points":[[100,200],[300,400]],"boxes":[[90,190,110,210],[290,390,310,410]]}"#;
        let line = execution_summary_for_tool_card(
            "mouse_click_at",
            &serde_json::json!({"target_desc": "勾选三项"}),
            Some(payload),
        )
        .expect("有 payload 就该出摘要");
        assert!(line.contains("2 calibrated points"), "{line}");
        assert!(line.contains("(100, 200)") && line.contains("[90, 190, 110, 210]"), "{line}");
        assert!(line.contains("(300, 400)") && line.contains("[290, 390, 310, 410]"), "{line}");
    }

    /// 单个 `point` 不能走多目标路径。
    #[test]
    fn single_point_does_not_use_the_multi_target_path() {
        let payload = r#"{"success":true,"action":"mouse_click_at","point":[605,307],"box":[588,291,612,309]}"#;
        let line = execution_summary_for_tool_card(
            "mouse_click_at",
            &serde_json::json!({"target_desc": "保存按钮"}),
            Some(payload),
        )
        .expect("有 payload 就该出摘要");
        assert!(!line.contains("calibrated points"), "单点走了多目标分支: {line}");
        assert!(line.contains("calibrated point (605, 307)"), "{line}");
    }

    /// 不能凭空断言「你传的 bbox」——模型可能压根没传，或者传了和这里无关。
    ///
    /// 摘要只陈述 payload 里的事实：落点和框。谁传的、差多少，payload 没有
    /// 这个信息，就不该写进摘要。
    #[test]
    fn mouse_summary_makes_no_claim_about_the_models_bbox() {
        let payload =
            r#"{"success":true,"action":"mouse_click_at","point":[605,307],"box":[588,291,612,309]}"#;
        for args in [
            serde_json::json!({"target_desc": "保存按钮", "bbox": [580, 280, 620, 320]}),
            serde_json::json!({"target_desc": "保存按钮"}),
            serde_json::json!({}),
        ] {
            let line = execution_summary_for_tool_card("mouse_click_at", &args, Some(payload))
                .expect("有 payload 就该出摘要");
            assert!(
                !line.to_lowercase().contains("you passed"),
                "摘要不该假设模型传过 bbox: {line}"
            );
        }
    }

    /// 没有 `box` 时只报落点，不能编一个框出来；没有 `point` 时也不能
    /// 保留「calibrated」这个形容词。
    #[test]
    fn mouse_summary_degrades_honestly() {
        let no_box = r#"{"success":true,"action":"mouse_click_at","point":[605,307]}"#;
        let line = execution_summary_for_tool_card("mouse_click_at", &serde_json::json!({}), Some(no_box))
            .expect("有 payload 就该出摘要");
        assert!(line.contains("(605, 307)"), "{line}");
        assert!(!line.contains("target box"), "没框就不该报框: {line}");

        let no_point = r#"{"success":true,"action":"mouse_click_at","box":[588,291,612,309]}"#;
        let line = execution_summary_for_tool_card("mouse_click_at", &serde_json::json!({}), Some(no_point))
            .expect("有 payload 就该出摘要");
        assert!(!line.contains("calibrated"), "没落点还叫 calibrated 是假的: {line}");
        assert!(line.contains("[588, 291, 612, 309]"), "框还是该报: {line}");
    }

    #[test]
    fn grounding_payload_is_recognised_without_matching_message_text() {
        // What `GroundFailure::to_payload` emits. Reading `success` + `status`
        // means rewording the Chinese message cannot silently re-enable verify
        // on a call that never clicked anything.
        let payload = r#"{"success":false,"status":"not_verified","target_desc":"E4 的红色9",
                          "your_bbox":[280,430,305,460],"rounds":3,
                          "failure_reason":"描述说框内有红色 9，但框里是空白"}"#;
        assert!(is_grounding_failure(Some(payload)));
    }

    #[test]
    fn other_failures_are_not_treated_as_grounding() {
        // `terminal` also reports `success: false`, but it ran and may have
        // changed the screen — its failure still goes through the normal path.
        let terminal = r#"{"success":false,"exitCode":1,"stdout":"","stderr":"boom"}"#;
        assert!(!is_grounding_failure(Some(terminal)));

        // A successful locate payload has no `success: false`.
        let ok = r#"{"success":true,"status":"located"}"#;
        assert!(!is_grounding_failure(Some(ok)));

        // Legacy `ERROR:` strings and plain text keep their old behaviour.
        assert!(!is_grounding_failure(Some("ERROR: Screen bounds not set")));
        assert!(!is_grounding_failure(None));
    }
}




