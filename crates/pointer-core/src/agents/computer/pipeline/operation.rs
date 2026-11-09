//! Map flat desktop tool names to operation families for position/verify prompt routing.
//! Spatial bridging (schema / merge / execution routing) lives in [`position_strategy`].

use crate::agents::computer::pipeline::position_strategy::{
    merge_position_output, resolve_execution_tool as strategy_resolve_execution_tool,
};
use crate::agents::computer::pipeline::types::PositionModuleOutput;
use serde_json::Value;

pub use crate::agents::computer::pipeline::position_strategy::{
    position_schema_for_family, PositionSpatialModel,
};

/// Operation family — one position prompt + one verify prompt per family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationFamily {
    PointerClick,
    PointerHover,
    Scroll,
    Drag,
    Input,
    ModifiedClick,
    Hotkey,
    Wait,
    Clipboard,
    AppAccess,
    Other,
}

impl OperationFamily {
    pub fn prompt_key(self) -> &'static str {
        match self {
            Self::PointerClick => "pointer_click",
            Self::PointerHover => "pointer_hover",
            Self::Scroll => "scroll",
            Self::Drag => "drag",
            Self::Input => "input",
            Self::ModifiedClick => "modified_click",
            Self::Hotkey => "hotkey",
            Self::Wait => "wait",
            Self::Clipboard => "clipboard",
            Self::AppAccess => "app_access",
            Self::Other => "generic",
        }
    }

    /// Whether the positioning LLM should run for this family.
    pub fn needs_positioning(self) -> bool {
        matches!(
            self,
            Self::PointerClick
                | Self::PointerHover
                | Self::Scroll
                | Self::Drag
                | Self::Input
                | Self::ModifiedClick
        )
    }

    /// Whether the verify LLM should run after execution.
    /// AppAccess uses host-only verify (OS API / tool text); Other has no verify prompt.
    pub fn needs_verify(self) -> bool {
        !matches!(self, Self::AppAccess | Self::Other)
    }
}

/// Classify a flat tool name into an operation family.
/// Supports both execution tool names (mouse_click_index, etc.) and decision tool names (click, hover, etc.).
pub fn operation_family_for_tool(tool_name: &str) -> OperationFamily {
    let n = tool_name.trim().to_ascii_lowercase();
    // Decision-phase tool names
    if n == "click" || n == "double_click" || n == "right_click" {
        return OperationFamily::PointerClick;
    }
    if n == "hover" || n == "mouse_move" {
        return OperationFamily::PointerHover;
    }
    if n == "scroll" {
        return OperationFamily::Scroll;
    }
    if n == "drag" {
        return OperationFamily::Drag;
    }
    if n == "input" || n == "input_focused" {
        return OperationFamily::Input;
    }
    if n == "modified_click" {
        return OperationFamily::ModifiedClick;
    }
    if n == "hotkey" {
        return OperationFamily::Hotkey;
    }
    if n == "wait" {
        return OperationFamily::Wait;
    }
    if n.starts_with("clipboard_") {
        return OperationFamily::Clipboard;
    }
    if n == "list_apps" || n == "launch_app" {
        return OperationFamily::AppAccess;
    }
    // Execution-phase tool names (legacy)
    if n.starts_with("mouse_click")
        || n.starts_with("mouse_double_click")
        || n.starts_with("mouse_right_click")
    {
        return OperationFamily::PointerClick;
    }
    // `move_to` is the tool id; `hover` is the method it maps to and the name
    // older traces carry.
    if n.starts_with("mouse_hover") || n == "move_to" {
        return OperationFamily::PointerHover;
    }
    if n.starts_with("mouse_scroll") {
        return OperationFamily::Scroll;
    }
    if n.starts_with("mouse_drag") {
        return OperationFamily::Drag;
    }
    if n.starts_with("input_") {
        return OperationFamily::Input;
    }
    if n.starts_with("modified_click") {
        return OperationFamily::ModifiedClick;
    }
    // Keyboard tools: `key_press` / `key_down` / `key_up` / `keyboard_type`.
    // They land keystrokes on whatever has focus, so they change the desktop and
    // need judging — `desktop_tool_family_id` already groups them under
    // "keyboard", this keeps the two classifications aligned.
    if n.starts_with("key_") || n.starts_with("keyboard_") {
        return OperationFamily::Hotkey;
    }
    OperationFamily::Other
}

/// Fields decision-phase tools must not emit (position module owns spatial + routing).
const DECISION_FORBIDDEN_KEYS: &[&str] = &[
    "x",
    "y",
    "index",
    "indices",
    "from_index",
    "to_index",
    "x1",
    "y1",
    "x2",
    "y2",
    "positions",
    "positioning_method",
    "positioning_route",
    "method",
];

/// Strip spatial parameters mistakenly emitted by the decision model.
pub fn sanitize_decision_tool_args(tool_name: &str, args: &Value) -> Value {
    if !crate::agents::computer::decision_tools::is_decision_tool(tool_name) {
        return args.clone();
    }
    let mut out = args.clone();
    let Some(obj) = out.as_object_mut() else {
        return out;
    };
    let stripped: Vec<&str> = DECISION_FORBIDDEN_KEYS
        .iter()
        .copied()
        .filter(|k| obj.remove(*k).is_some())
        .collect();
    if !stripped.is_empty() {
        log::warn!(
            "computer pipeline: stripped decision spatial fields {stripped:?} from tool={tool_name}"
        );
    }
    out
}

/// Merge position module JSON fields into decision tool arguments.
pub fn merge_position_into_tool_args(args: &mut Value, pos: &PositionModuleOutput) {
    let family = args
        .get("_pipeline_family_hint")
        .and_then(|v| v.as_str())
        .map(operation_family_for_tool)
        .unwrap_or(OperationFamily::PointerClick);
    merge_position_output(family, args, pos);
}

/// Merge using an explicit family (preferred in pipeline).
pub fn merge_position_into_tool_args_for_family(
    family: OperationFamily,
    args: &mut Value,
    pos: &PositionModuleOutput,
) {
    merge_position_output(family, args, pos);
}

/// Map a decision-phase tool name + position output to an execution-phase tool name.
pub fn resolve_execution_tool(decision_name: &str, pos: &PositionModuleOutput) -> String {
    strategy_resolve_execution_tool(decision_name, pos)
}
/// Decide the positioning route from args.
/// Decision tools never emit routing — always `auto` (position module decides).
/// Execution tools infer from tool name suffix.
pub fn positioning_route(tool_name: &str, args: &Value) -> &'static str {
    if crate::agents::computer::decision_tools::is_decision_tool(tool_name) {
        return "auto";
    }
    if let Some(method) = args.get("positioning_method").and_then(|v| v.as_str()) {
        return match method {
            "at" | "at_xy" | "coordinate" | "coords" => "at",
            "index" => "index",
            "none" | "skip" => "none",
            _ => "auto",
        };
    }
    let n = tool_name.trim().to_ascii_lowercase();
    if n.contains("_at") {
        "at"
    } else if n.contains("_index") {
        "index"
    } else {
        "auto"
    }
}

/// Build execution-phase tool args from decision-phase args + position output.
pub fn build_execution_args(
    decision_name: &str,
    decision_args: &Value,
    pos: &PositionModuleOutput,
) -> Value {
    let family = operation_family_for_tool(decision_name);
    let mut args = decision_args.clone();
    if let Some(obj) = args.as_object_mut() {
        obj.remove("positioning_method");
        if !obj.contains_key("goal") {
            if let Some(action_val) = obj.get("action").cloned() {
                obj.insert("goal".into(), action_val);
            }
        }
    }
    merge_position_output(family, &mut args, pos);
    args
}

/// `goal` 已并入 `action`。执行层的参数校验（`require_non_empty_str(args,
/// "goal")`）、卡片标题、`[Recent desktop tool calls]` 都还在读 `goal`，所以
/// 在工具调用入口把 `action` 补成 `goal`，下游不必逐个改。
///
/// 只对 desktop 工具生效 —— `job` / `cron_job` 也有 `action` 字段（`list` /
/// `create` 这种枚举），不能把它们的 `action` 当成 `goal`。
/// `goal` 已经存在时保持原值（旧会话的历史行、以及模型偶尔仍写 `goal` 的情况）。
pub fn ensure_goal_from_action(tool_id: &str, args: &mut Value) {
    if !crate::agents::computer::is_desktop_vision_log_tool(tool_id) {
        return;
    }
    let Some(obj) = args.as_object_mut() else {
        return;
    };
    let goal_present = obj
        .get("goal")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty());
    if goal_present {
        return;
    }
    let action = obj
        .get("action")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if let Some(action) = action {
        obj.insert("goal".into(), Value::String(action));
    }
}

/// Decide the positioning route from args.
pub fn needs_positioning_for_tool(tool_name: &str, args: &serde_json::Value) -> bool {
    let n = tool_name.trim().to_ascii_lowercase();
    // Non-spatial tools that never need positioning
    if matches!(
        n.as_str(),
        "wait"
            | "hotkey"
            | "input_focused"
            | "clipboard_read"
            | "clipboard_write"
            | "mouse_scroll_current"
            | "list_apps"
            | "launch_app"
    ) {
        return false;
    }
    // Check positioning route override
    match positioning_route(tool_name, args) {
        "none" => return false,
        "auto" => {}
        _ => return true,
    }
    operation_family_for_tool(tool_name).needs_positioning()
}

/// Whether verify LLM should run after execution for this tool.
pub fn needs_verify_for_tool(tool_name: &str, _args: &Value) -> bool {
    operation_family_for_tool(tool_name).needs_verify()
}

/// Summarize an operation for decision/verify context injection.
pub fn format_operation_summary(tool_name: &str, args: &Value) -> String {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    // `goal` may be absent in decision-phase args; fall back to action.
    let goal = args.get("goal").and_then(|v| v.as_str()).unwrap_or(action);
    format!("tool={tool_name} goal=\"{goal}\" action=\"{action}\"")
}

#[cfg(test)]
mod tests {

    /// 新桌面工具必须纳入 `action → goal` 的归一化，否则执行层会报缺 goal。
    /// `use_screen` 加进来时漏过一次，这里钉住。
    #[test]
    fn use_screen_gets_goal_from_action() {
        let mut args = serde_json::json!({ "action": "切到 Chrome 所在的屏" });
        ensure_goal_from_action("use_screen", &mut args);
        assert_eq!(args["goal"], "切到 Chrome 所在的屏");
    }

    /// 非桌面工具不能被误补：`job` 的 `action` 是 `list`/`create` 这类枚举。
    #[test]
    fn non_desktop_tool_action_is_not_turned_into_goal() {
        let mut args = serde_json::json!({ "action": "list" });
        ensure_goal_from_action("job", &mut args);
        assert!(args.get("goal").is_none());
    }

    use super::*;
    use crate::agents::computer::pipeline::types::{PositionModuleOutput, PositionPoint};
    use serde_json::json;

    #[test]
    fn classifies_mouse_click_at() {
        assert_eq!(
            operation_family_for_tool("mouse_click_at"),
            OperationFamily::PointerClick
        );
    }

    #[test]
    fn wait_skips_positioning() {
        assert!(!OperationFamily::Wait.needs_positioning());
    }

    #[test]
    fn input_needs_positioning_for_at_variant() {
        assert!(OperationFamily::Input.needs_positioning());
    }

    #[test]
    fn app_access_skips_verify_llm() {
        assert!(!OperationFamily::AppAccess.needs_verify());
        assert!(OperationFamily::Input.needs_verify());
        assert!(OperationFamily::PointerClick.needs_verify());
    }

    #[test]
    fn app_access_skips_positioning() {
        assert_eq!(
            operation_family_for_tool("launch_app"),
            OperationFamily::AppAccess
        );
        assert!(!needs_positioning_for_tool(
            "launch_app",
            &json!({"goal": "open Safari", "app": "Safari"})
        ));
    }

    #[test]
    fn merge_at_route_writes_xy_not_reference_index_as_target() {
        let mut args = json!({"goal": "g"});
        let pos = PositionModuleOutput {
            submit_route: Some(crate::agents::computer::pipeline::types::PositionSubmitRoute::At),
            x: Some(279),
            y: Some(332),
            ..Default::default()
        };
        merge_position_into_tool_args(&mut args, &pos);
        assert_eq!(args["x"], 279);
        assert_eq!(args["y"], 332);
        assert!(args.get("index").is_none());
    }

    #[test]
    fn resolve_execution_click_at_route() {
        let pos = PositionModuleOutput {
            submit_route: Some(crate::agents::computer::pipeline::types::PositionSubmitRoute::At),
            x: Some(279),
            y: Some(332),
            ..Default::default()
        };
        assert_eq!(resolve_execution_tool("click", &pos), "mouse_click_at");
    }

    #[test]
    fn sanitize_strips_decision_spatial_and_routing_fields() {
        let raw = json!({
            "action": "Click copy icon",
            "indices": [0],
            "index": 99,
            "x": 1,
            "y": 2,
            "positioning_method": "index"
        });
        let clean = sanitize_decision_tool_args("modified_click", &raw);
        assert_eq!(clean["action"], "Click copy icon");
        assert!(clean.get("indices").is_none());
        assert!(clean.get("index").is_none());
        assert!(clean.get("x").is_none());
        assert!(clean.get("positioning_method").is_none());
    }

    #[test]
    fn classifies_mouse_move_as_pointer_hover() {
        assert_eq!(
            operation_family_for_tool("mouse_move"),
            OperationFamily::PointerHover
        );
    }

    #[test]
    fn mouse_move_needs_positioning() {
        assert!(needs_positioning_for_tool(
            "mouse_move",
            &json!({
                "action": "Move cursor to scroll region"
            })
        ));
    }

    #[test]
    fn resolve_execution_mouse_move_goes_to_move_to() {
        let pos = PositionModuleOutput {
            x: Some(12),
            y: Some(34),
            ..Default::default()
        };
        assert_eq!(resolve_execution_tool("mouse_move", &pos), "move_to");
    }

    #[test]
    fn decision_tool_always_needs_positioning() {
        assert!(needs_positioning_for_tool(
            "click",
            &json!({"action": "Click the Save button"})
        ));
    }

    #[test]
    fn decision_click_always_auto_positioning_route() {
        assert_eq!(
            positioning_route("click", &json!({"positioning_method": "at"})),
            "auto"
        );
        assert_eq!(
            positioning_route("click", &json!({"positioning_method": "index"})),
            "auto"
        );
        assert_eq!(positioning_route("click", &json!({})), "auto");
    }

    #[test]
    fn execution_tool_infers_index_route_from_name() {
        assert_eq!(positioning_route("mouse_click_at", &json!({})), "at");
    }

    #[test]
    fn resolve_execution_click_with_coordinates() {
        let pos = PositionModuleOutput {
            x: Some(5),
            y: Some(6),
            ..Default::default()
        };
        assert_eq!(resolve_execution_tool("click", &pos), "mouse_click_at");
    }

    #[test]
    fn resolve_execution_click_without_index() {
        let pos = PositionModuleOutput::default();
        assert_eq!(resolve_execution_tool("click", &pos), "mouse_click_at");
    }

    #[test]
    fn resolve_execution_nonspatial_passthrough() {
        let pos = PositionModuleOutput::default();
        assert_eq!(resolve_execution_tool("hotkey", &pos), "hotkey");
    }

    #[test]
    fn resolve_execution_drag_with_from_to() {
        let pos = PositionModuleOutput {
            positions: Some(vec![
                crate::agents::computer::pipeline::types::PositionPoint { x: 3, y: 4 },
                crate::agents::computer::pipeline::types::PositionPoint { x: 7, y: 8 },
            ]),
            ..Default::default()
        };
        assert_eq!(
            resolve_execution_tool("drag", &pos),
            "mouse_drag_from_to_at"
        );
    }

    #[test]
    fn positioning_route_from_decision_tool() {
        assert_eq!(positioning_route("click", &json!({})), "auto");
    }

    #[test]
    fn ensure_goal_from_action_fills_missing_goal() {
        // 鼠标 schema 现在只有 `action`。执行层的 `require_non_empty_str(args,
        // "goal")` 还在，入口补不上的话每次点击都会报参数错误。
        let mut args = json!({"action": "点击保存", "bbox": [1, 2, 3, 4]});
        ensure_goal_from_action("mouse_click_at", &mut args);
        assert_eq!(args["goal"], "点击保存");
    }

    #[test]
    fn ensure_goal_from_action_keeps_existing_goal() {
        let mut args = json!({"goal": "打开设置", "action": "click"});
        ensure_goal_from_action("mouse_click_at", &mut args);
        assert_eq!(args["goal"], "打开设置");
    }

    #[test]
    fn ensure_goal_from_action_ignores_non_desktop_tools() {
        // `job` 的 `action` 是枚举（list/status/await/cancel），不能变成 goal。
        let mut args = json!({"action": "list"});
        ensure_goal_from_action("job", &mut args);
        assert!(args.get("goal").is_none(), "{args}");
        let mut cron = json!({"action": "create", "schedule": "0 9 * * *"});
        ensure_goal_from_action("cron_job", &mut cron);
        assert!(cron.get("goal").is_none(), "{cron}");
    }

    #[test]
    fn ensure_goal_from_action_ignores_blank_action() {
        let mut args = json!({"action": "   "});
        ensure_goal_from_action("key_press", &mut args);
        assert!(args.get("goal").is_none(), "{args}");
    }

    #[test]
    fn build_execution_args_adds_goal_from_action() {
        let args = json!({"action": "Click Save"});
        let pos = PositionModuleOutput::default();
        let exec = build_execution_args("click", &args, &pos);
        // Execution tools need `goal` — should be auto-filled from `action`
        assert_eq!(exec["goal"], "Click Save");
        assert_eq!(exec["action"], "Click Save");
    }

    #[test]
    fn build_execution_args_maps_positions_for_modified_click() {
        let args = json!({"action": "Ctrl+click row 3"});
        let pos = PositionModuleOutput {
            positions: Some(vec![PositionPoint { x: 42, y: 43 }]),
            ..Default::default()
        };
        let exec = build_execution_args("modified_click", &args, &pos);
        assert_eq!(exec["positions"], json!([{"x": 42, "y": 43}]));
        assert!(exec.get("index").is_none());
        assert!(exec.get("indices").is_none());
    }

    #[test]
    fn build_execution_args_maps_positions_for_modified_click_at() {
        let args = json!({"action": "Ctrl+click icon"});
        let pos = PositionModuleOutput {
            positions: Some(vec![PositionPoint { x: 100, y: 200 }]),
            ..Default::default()
        };
        let exec = build_execution_args("modified_click", &args, &pos);
        assert_eq!(exec["positions"], json!([{"x": 100, "y": 200}]));
    }

    #[test]
    fn keyboard_tools_need_verify() {
        // `desktop_tool_family_id` groups these under "keyboard", so they are
        // recorded on history rows. Classification has to agree, otherwise the
        // row exists but no verify ever closes it.
        for name in ["key_press", "key_down", "key_up", "keyboard_type"] {
            assert_eq!(
                operation_family_for_tool(name),
                OperationFamily::Hotkey,
                "{name} should classify as a keyboard action"
            );
            assert!(
                needs_verify_for_tool(name, &Value::Null),
                "{name} should need verify"
            );
        }
    }

    #[test]
    fn classification_covers_every_vision_logged_desktop_tool() {
        // The two classifications must agree on which tools touch the desktop.
        // A tool that `desktop_tool_family_id` logs but `operation_family_for_tool`
        // calls `Other` gets a history row that nothing ever verifies.
        for name in [
            "mouse_click_at",
            "move_to",
            "mouse_scroll_at",
            "mouse_drag_from_to_at",
            "input_at",
            "modified_click_at",
            "hotkey",
            "wait",
            "clipboard_read",
            "clipboard_write",
            "key_press",
            "keyboard_type",
        ] {
            assert!(
                crate::agents::computer::input::timing::is_desktop_vision_log_tool(name),
                "{name} should be logged as a desktop tool"
            );
            assert_ne!(
                operation_family_for_tool(name),
                OperationFamily::Other,
                "{name} is logged but classifies as Other — its verify row would never close"
            );
        }
    }
}
