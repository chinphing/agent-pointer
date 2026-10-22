//! Mouse tools async dispatch — click/hover/drag by description.
//!
//! These live on the async path for one reason: when the caller passes
//! `description` instead of `x`/`y`, the tool has to run visual grounding first,
//! and grounding needs the agent's provider. The synchronous `ToolHandler`
//! closure only carries `Arc<ComputerState>`, so it can capture a screenshot but
//! cannot call a vision model.
//!
//! Both forms stay supported:
//!   {description: "搜索框"}   → locate, then click the returned centre
//!   {x: 798, y: 272}          → click directly, no vision call
//!
//! Everything else (validation, human_like, wait, coordinate system) is still
//! handled by `MouseTool`, so behaviour is identical once the point is known.

use crate::agents::computer::tools::tool_mouse::MouseTool;
use crate::agents::computer::tools::MouseBackend;
use crate::models::ToolCall;
use crate::provider::OpenAIProvider;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::super::super::app_state::AppState;
use super::super::types::ToolExecResult;

/// `mouse_*` tools that accept a `description` instead of `x`/`y`.
///
/// `mouse_scroll_current` is absent on purpose: it scrolls at the current
/// pointer, so there is nothing to locate.
pub(super) const DESCRIPTION_CAPABLE: &[&str] = &[
    "mouse_click_at",
    "mouse_double_click_at",
    "mouse_right_click_at",
    "mouse_hover_at",
    "mouse_drag_to",
    "mouse_down",
    "mouse_up",
];

/// Map a tool name to the `MouseBackend::At` method name.
fn method_for(tool_id: &str) -> Option<&'static str> {
    Some(match tool_id {
        "mouse_click_at" => "click",
        "mouse_double_click_at" => "double_click",
        "mouse_right_click_at" => "right_click",
        "mouse_hover_at" => "hover",
        "mouse_drag_to" => "drag_to",
        "mouse_down" => "mouse_down",
        "mouse_up" => "mouse_up",
        _ => return None,
    })
}

/// Resolve `description` → `(x, y)` when the caller did not supply coordinates.
///
/// Returns `Ok(None)` when `x`/`y` are already present, so the caller can skip
/// the vision call entirely.
async fn resolve_point(
    state: &AppState,
    conversation_id: &str,
    provider: &OpenAIProvider,
    args: &Value,
    cancel: &CancellationToken,
) -> anyhow::Result<Option<(f64, f64)>> {
    let has_xy = args.get("x").and_then(|v| v.as_f64()).is_some()
        && args.get("y").and_then(|v| v.as_f64()).is_some();
    if has_xy {
        return Ok(None);
    }
    let description = args
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(description) = description else {
        anyhow::bail!(
            "要么给 description（自动定位），要么给 x 和 y（直接点）——两个都没给"
        );
    };

    // Same provider resolution as `dispatch_locate_element`: grounding may be
    // pinned to its own model, and the key has to come from that provider.
    let phase_settings = {
        let cfg = state.computer_state.effective_tier_config();
        let model = cfg.pipeline_llm.model_locate.trim();
        if model.is_empty() {
            provider.settings.clone()
        } else {
            state.computer_state.apply_pipeline_phase_settings(
                conversation_id,
                crate::agents::computer::tier::PipelineLlmPhase::Locate,
                &provider.settings,
            )
        }
    };
    let locate_key = crate::provider::api_key_for_settings(&phase_settings)?;
    let locate_provider = OpenAIProvider::new(phase_settings, locate_key);

    let locate_args = serde_json::json!({
        "goal": format!("点击{description}"),
        "elements": [{ "description": description }]
    });
    let payload = crate::agents::computer::locate::run_locate_element(
        state.computer_state.as_ref(),
        conversation_id,
        &locate_provider,
        &locate_args,
        cancel,
    )
    .await?;

    let parsed: Value = serde_json::from_str(&payload)
        .map_err(|e| anyhow::anyhow!("locate 返回的不是合法 JSON: {e}"))?;
    let first = parsed
        .get("results")
        .and_then(|r| r.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| anyhow::anyhow!("locate 没有返回结果"))?;

    let status = first.get("status").and_then(|v| v.as_str()).unwrap_or("");
    if status != "located" {
        // Surface the locate failure verbatim — its `summary` already explains
        // what went wrong in user terms (distance, what was actually in the box).
        let summary = first
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("定位失败");
        anyhow::bail!("没找到「{description}」：{summary}");
    }
    let x = first
        .get("x")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| anyhow::anyhow!("locate 结果缺少 x"))?;
    let y = first
        .get("y")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| anyhow::anyhow!("locate 结果缺少 y"))?;
    Ok(Some((x, y)))
}

pub(super) async fn dispatch_mouse(
    state: &AppState,
    conversation_id: &str,
    provider: &OpenAIProvider,
    tc: &ToolCall,
    args_value: Value,
    cancel: &CancellationToken,
) -> ToolExecResult {
    let tool_id = tc.name.as_str();
    let Some(method) = method_for(tool_id) else {
        return Err(anyhow::anyhow!("dispatch_mouse: 未知工具 {tool_id}"));
    };

    let mut args = args_value;
    let point = resolve_point(state, conversation_id, provider, &args, cancel).await?;

    if let Some((x, y)) = point {
        // Write the resolved point back so `MouseTool` validates it exactly like
        // a caller-supplied coordinate (bounds, coordinate system, human_like).
        let obj = args
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("mouse 参数必须是对象"))?;
        obj.insert("x".to_string(), serde_json::json!(x));
        obj.insert("y".to_string(), serde_json::json!(y));
        // `description` is ours, not MouseTool's — drop it so it cannot confuse
        // argument parsing downstream.
        obj.remove("description");
    }

    let vision = state
        .computer_state
        .vision_state_for_conversation(conversation_id);
    let tool = MouseTool::new(state.computer_state.control.clone(), vision);
    let out = tool.execute_with(MouseBackend::At, method, &args)?;
    Ok((out, true, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_description_capable_tool_has_a_method() {
        for name in DESCRIPTION_CAPABLE {
            assert!(method_for(name).is_some(), "{name} 没有对应的 method");
        }
    }

    #[test]
    fn scroll_is_not_description_capable() {
        assert!(!DESCRIPTION_CAPABLE.contains(&"mouse_scroll_current"));
        assert!(method_for("mouse_scroll_current").is_none());
    }
}
