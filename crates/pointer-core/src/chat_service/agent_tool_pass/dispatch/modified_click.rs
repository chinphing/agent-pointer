//! `modified_click_*` async dispatch — multi-select by described targets.
//!
//! Both tools take `positions: [{target_desc, bbox}, ...]` and ground each entry
//! independently. A multi-select is a list of *things*, not a list of points:
//! grounding each one is what makes "select these three rows" mean those three
//! rows rather than three places that happen to line up.
//!
//! On the async path for the same reason as `dispatch_mouse` — grounding needs
//! the agent's provider.

use crate::agents::computer::tools::args_util::effective_human_like_default;
use crate::agents::computer::tools::tool_modified_click::ModifiedClickTool;
use crate::agents::computer::tools::ModifiedClickBackend;
use crate::models::ToolCall;
use crate::provider::OpenAIProvider;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::super::super::app_state::AppState;
use super::super::types::ToolExecResult;
use super::grounding::{resolve_point, success_payload, GroundOutcome};

fn method_for(tool_id: &str) -> Option<&'static str> {
    Some(match tool_id {
        "modified_click_select_at" => "select",
        "modified_click_range_select_at" => "range_select",
        _ => return None,
    })
}

pub(super) async fn dispatch_modified_click(
    state: &AppState,
    conversation_id: &str,
    provider: &OpenAIProvider,
    tc: &ToolCall,
    args_value: Value,
    cancel: &CancellationToken,
) -> ToolExecResult {
    let tool_id = tc.name.as_str();
    let Some(method) = method_for(tool_id) else {
        return Err(anyhow::anyhow!(
            "dispatch_modified_click: 未知工具 {tool_id}"
        ));
    };

    let entries = args_value
        .get("positions")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("缺少 positions 数组"))?
        .clone();
    if entries.is_empty() {
        return Err(anyhow::anyhow!("positions 不能为空"));
    }

    // Ground every entry before touching the mouse: a half-applied selection
    // (three of five rows) is worse than none, because the next step builds on
    // a state the model believes it created.
    let mut points: Vec<(f64, f64)> = Vec::with_capacity(entries.len());
    let mut boxes: Vec<Value> = Vec::with_capacity(entries.len());
    for (i, entry) in entries.iter().enumerate() {
        // `positions[i]` is grounded on its own: one bad entry fails the whole
        // call, and the payload says which index it was.
        let (x, y, bbox) =
            match resolve_point(state, conversation_id, provider, entry, "", cancel).await? {
                GroundOutcome::Point { x, y, bbox } => (x, y, bbox),
                GroundOutcome::Failed(mut f) => {
                    f.status = format!("{} (positions[{i}])", f.status);
                    return Ok((f.to_payload(), false, Some(f.failure_reason.clone())));
                }
            };
        points.push((x, y));
        boxes.push(bbox);
    }

    let mut args = args_value;
    {
        let obj = args
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("modified_click 参数必须是对象"))?;
        // Downstream expects the resolved form: [x, y] pairs.
        obj.insert(
            "positions".to_string(),
            Value::Array(
                points
                    .iter()
                    .map(|(x, y)| json!([x, y]))
                    .collect::<Vec<_>>(),
            ),
        );
    }

    // 容器会话：多选点击必须打在容器桌面。宿主 executor 是 Enigo，够不到容器里的
    // X 桌面——真跑起来会在 Mac 上乱点，而 `[CUR_SCREEN]` 显示的是容器，看不出错。
    if let Some(env) = crate::env::session_env::env_for_conversation(conversation_id) {
        if env.kind() == crate::env::EnvKind::Container {
            use crate::agents::computer::control::{ComputerControl, DockerControl};
            use crate::agents::computer::input::actions::MouseButton;

            let control = DockerControl::new(env);
            let out = match method {
                "select" => {
                    for (x, y) in &points {
                        control.key_down("ctrl")?;
                        control.click(MouseButton::Left, Some(*x as i32), Some(*y as i32), 1)?;
                        control.key_up("ctrl")?;
                    }
                    format!(
                        "Cmd/Ctrl+click {} position(s). Verify on next screenshot.",
                        points.len()
                    )
                }
                _ => {
                    let first = points.first().copied().unwrap_or((0.0, 0.0));
                    let last = points.last().copied().unwrap_or(first);
                    control.click(MouseButton::Left, Some(first.0 as i32), Some(first.1 as i32), 1)?;
                    control.key_down("shift")?;
                    control.click(MouseButton::Left, Some(last.0 as i32), Some(last.1 as i32), 1)?;
                    control.key_up("shift")?;
                    "Shift+click range by coordinates. Verify result on next screenshot.".to_string()
                }
            };
            return Ok((success_payload(tool_id, &points, &boxes, &out), true, None));
        }
    }

    let vision = state
        .computer_state
        .vision_state_for_conversation(conversation_id);
    let tool = ModifiedClickTool::new(
        state.computer_state.executor.clone(),
        vision,
        effective_human_like_default(),
    );
    let out = tool.execute_with(ModifiedClickBackend::At, method, &args)?;
    // Same envelope as the failure path: `success` plus every grounded point and
    // the box each one landed in.
    Ok((
        success_payload(tool_id, &points, &boxes, &out),
        true,
        None,
    ))
}
