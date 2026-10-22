//! Mouse tools async dispatch — click/hover/drag by target description.
//!
//! Every pointing tool now takes `{target_desc, bbox}` and nothing else. The
//! pair is not redundant: `bbox` is where the caller thinks the target is,
//! `target_desc` is what it is. Grounding uses the box as its search seed and
//! the description as the acceptance criterion, so a box that is off by a few
//! dozen pixels — which is what the model's own estimates look like — still
//! lands on the right element instead of clicking the neighbour.
//!
//! This runs on the async path because grounding needs the agent's provider,
//! and the synchronous `ToolHandler` closure only carries `Arc<ComputerState>`.
//!
//! Callers get the box from `locate_element`, which returns `bbox` for exactly
//! this purpose. There is no raw-coordinate form: an unchecked `x`/`y` click has
//! no semantic confirmation behind it, and that is how a click lands on a
//! same-named element two rows away.

use crate::agents::computer::tools::tool_mouse::MouseTool;
use crate::agents::computer::tools::MouseBackend;
use crate::models::ToolCall;
use crate::provider::OpenAIProvider;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::super::super::app_state::AppState;
use super::super::types::ToolExecResult;

/// `mouse_*` tools that point at a described target.
///
/// `mouse_scroll_current` is absent: it scrolls at the current pointer, so
/// there is nothing to ground.
///
/// `mouse_drag_from_to_at` is the odd one out: it needs two points (grab, drop),
/// so it takes `from_target_desc`/`from_bbox` and `to_target_desc`/`to_bbox`
/// instead of the single pair. Both ends are grounded independently.
/// Kept as documentation of the set, and asserted in tests; the dispatch
/// itself goes through `method_for`.
#[cfg(test)]
pub(super) const TARGETED: &[&str] = &[
    "mouse_click_at",
    "mouse_double_click_at",
    "mouse_right_click_at",
    "mouse_hover_at",
    "mouse_drag_to",
    "mouse_down",
    "mouse_up",
    "mouse_drag_from_to_at",
];

/// Map a tool name to the `MouseBackend::At` method name.
fn method_for(tool_id: &str) -> Option<&'static str> {
    Some(match tool_id {
        "mouse_click_at" => "click",
        "mouse_double_click_at" => "double_click",
        "mouse_right_click_at" => "right_click",
        "mouse_hover_at" => "hover",
        "mouse_drag_to" => "drag_to",
        "mouse_drag_from_to_at" => "drag_from_to",
        "mouse_down" => "mouse_down",
        "mouse_up" => "mouse_up",
        _ => return None,
    })
}

/// `[x1, y1, x2, y2]` → `PxBox`, rejecting degenerate boxes.
pub(super) fn parse_bbox(v: &Value) -> anyhow::Result<crate::agents::computer::locate::PxBox> {
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("bbox 必须是 [x1, y1, x2, y2]"))?;
    if arr.len() != 4 {
        anyhow::bail!("bbox 必须是 4 个数，收到 {}", arr.len());
    }
    let mut n = [0f64; 4];
    for (i, item) in arr.iter().enumerate() {
        n[i] = item
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("bbox[{i}] 不是数字"))?;
    }
    let b = crate::agents::computer::locate::PxBox {
        x1: n[0],
        y1: n[1],
        x2: n[2],
        y2: n[3],
    };
    if b.x2 <= b.x1 || b.y2 <= b.y1 {
        anyhow::bail!(
            "bbox 宽高必须为正：收到 [{}, {}, {}, {}]",
            n[0],
            n[1],
            n[2],
            n[3]
        );
    }
    Ok(b)
}

/// Ground `{target_desc, bbox}` and return the confirmed centre point.
pub(super) async fn resolve_point(
    state: &AppState,
    conversation_id: &str,
    provider: &OpenAIProvider,
    args: &Value,
    prefix: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<(f64, f64)> {
    let desc_key = format!("{prefix}target_desc");
    let bbox_key = format!("{prefix}bbox");
    let target_desc = args
        .get(&desc_key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("缺少 {desc_key}——目标是什么"))?;
    let bbox_value = args
        .get(&bbox_key)
        .ok_or_else(|| anyhow::anyhow!("缺少 {bbox_key}——目标大概在哪个范围"))?;
    let hint = parse_bbox(bbox_value)?;

    let locate_provider = super::grounding::locate_provider(state, conversation_id, provider)?;

    let locate_args = serde_json::json!({
        "goal": format!("点击{target_desc}"),
        "elements": [{
            "description": target_desc,
            "hint_box": [hint.x1, hint.y1, hint.x2, hint.y2],
        }]
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
        // what went wrong in user terms.
        let summary = first
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("定位失败");
        anyhow::bail!("没找到「{target_desc}」：{summary}");
    }
    let confirmed = first
        .get("bbox")
        .ok_or_else(|| anyhow::anyhow!("locate 结果缺少 bbox"))
        .and_then(parse_bbox)?;
    let (cx, cy) = confirmed.center();
    Ok((cx, cy))
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
    if tool_id == "mouse_drag_from_to_at" {
        // Two ends, grounded separately: the grab point and the drop point can
        // be far apart and are often different kinds of thing.
        let (fx, fy) = resolve_point(state, conversation_id, provider, &args, "from_", cancel).await?;
        let (tx, ty) = resolve_point(state, conversation_id, provider, &args, "to_", cancel).await?;
        let obj = args
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("mouse 参数必须是对象"))?;
        obj.insert("x1".to_string(), serde_json::json!(fx));
        obj.insert("y1".to_string(), serde_json::json!(fy));
        obj.insert("x2".to_string(), serde_json::json!(tx));
        obj.insert("y2".to_string(), serde_json::json!(ty));
        for k in ["from_target_desc", "from_bbox", "to_target_desc", "to_bbox"] {
            obj.remove(k);
        }
    } else {
        let (x, y) = resolve_point(state, conversation_id, provider, &args, "", cancel).await?;
        let obj = args
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("mouse 参数必须是对象"))?;
        obj.insert("x".to_string(), serde_json::json!(x));
        obj.insert("y".to_string(), serde_json::json!(y));
        // These are ours, not MouseTool's — drop them so argument parsing
        // downstream sees exactly the shape it expects.
        obj.remove("target_desc");
        obj.remove("bbox");
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
    use serde_json::json;

    #[test]
    fn every_targeted_tool_has_a_method() {
        for name in TARGETED {
            assert!(method_for(name).is_some(), "{name} 没有对应的 method");
        }
    }

    #[test]
    fn scroll_is_not_targeted() {
        assert!(!TARGETED.contains(&"mouse_scroll_current"));
        assert!(method_for("mouse_scroll_current").is_none());
    }

    #[test]
    fn drag_from_to_uses_two_ends() {
        // 两对参数：from_* 和 to_*，不是一个 target_desc 能描述的
        assert!(TARGETED.contains(&"mouse_drag_from_to_at"));
        assert_eq!(method_for("mouse_drag_from_to_at"), Some("drag_from_to"));
    }

    #[test]
    fn parse_bbox_rejects_bad_shapes() {
        assert!(parse_bbox(&json!([10, 20, 30, 40])).is_ok());
        assert!(parse_bbox(&json!([10, 20, 30])).is_err(), "3 个数");
        assert!(parse_bbox(&json!([10, 20, 30, 40, 50])).is_err(), "5 个数");
        assert!(parse_bbox(&json!("10,20,30,40")).is_err(), "字符串");
        assert!(parse_bbox(&json!([30, 20, 10, 40])).is_err(), "x2 < x1");
        assert!(parse_bbox(&json!([10, 40, 30, 20])).is_err(), "y2 < y1");
        assert!(parse_bbox(&json!([10, 20, 10, 40])).is_err(), "零宽");
    }

    #[test]
    fn parse_bbox_keeps_the_centre() {
        let b = parse_bbox(&json!([100, 200, 300, 260])).unwrap();
        assert_eq!(b.center(), (200.0, 230.0));
    }
}
