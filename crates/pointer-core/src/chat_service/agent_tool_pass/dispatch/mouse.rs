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
use super::grounding::resolve_point;

/// `mouse_*` tools that **always** require `target_desc` + `bbox`.
///
/// `mouse_scroll_current` is absent because its target is optional: without
/// `target_desc` it scrolls at the current pointer. With one, `dispatch_mouse`
/// grounds it and the pointer moves before the scroll — but that is a per-call
/// choice, so it cannot join this set.
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
        "mouse_drag_from_to_at" => "drag_from_to",
        "mouse_down" => "mouse_down",
        "mouse_up" => "mouse_up",
        "mouse_scroll_current" => "scroll_current",
        _ => return None,
    })
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
    } else if tool_id == "mouse_scroll_current" {
        // `target_desc` / `bbox` are optional here: without them this keeps the
        // old "scroll at the current pointer" behaviour. With them, ground the
        // panel and hand `x`/`y` to `MouseTool`, which moves the pointer before
        // scrolling — wheel events only reach the window under the pointer.
        let has_target = args
            .get("target_desc")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty());
        if has_target {
            let (x, y) = resolve_point(state, conversation_id, provider, &args, "", cancel).await?;
            let obj = args
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("mouse 参数必须是对象"))?;
            obj.insert("x".to_string(), serde_json::json!(x));
            obj.insert("y".to_string(), serde_json::json!(y));
            obj.remove("target_desc");
            obj.remove("bbox");
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
    fn scroll_is_dispatched_but_not_in_the_always_targeted_set() {
        // `mouse_scroll_current` now grounds an optional panel, so it must reach
        // a method — but `target_desc`/`bbox` stay optional, so it is not part of
        // the always-targeted set that requires them.
        assert!(!TARGETED.contains(&"mouse_scroll_current"));
        assert_eq!(method_for("mouse_scroll_current"), Some("scroll_current"));
    }

    #[test]
    fn drag_from_to_uses_two_ends() {
        // 两对参数：from_* 和 to_*，不是一个 target_desc 能描述的
        assert!(TARGETED.contains(&"mouse_drag_from_to_at"));
        assert_eq!(method_for("mouse_drag_from_to_at"), Some("drag_from_to"));
    }

}
