//! Route unified computer tool `method` labels to internal backends.
//!
//! `action` in tool args is the **human target description** (shown in UI).
//! `method` is the **operation name** (`click_index`, `click_at`, …).
//! Do not treat natural-language `action` as the operation.

use anyhow::{anyhow, Result};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseBackend {
    Index,
    At,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputBackend {
    Index,
    At,
    Focused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifiedClickBackend {
    Index,
    At,
}

pub struct RoutedMouse {
    pub backend: MouseBackend,
    pub method: String,
}

pub struct RoutedInput {
    pub backend: InputBackend,
    pub method: String,
}

pub struct RoutedModifiedClick {
    pub backend: ModifiedClickBackend,
    pub method: String,
}

fn has_xy(args: &Value) -> bool {
    args.get("x").is_some() && args.get("y").is_some()
}

fn has_index_targeting(args: &Value) -> bool {
    args.get("index").is_some()
        || args.get("from_index").is_some()
        || args.get("to_index").is_some()
        || args.get("indices").is_some()
}

fn strip_suffix(name: &str, suffix: &str) -> String {
    name.strip_suffix(suffix).unwrap_or(name).to_string()
}

/// True when `s` looks like an ASCII operation name, not a human target description.
fn looks_like_operation_name(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || !s.is_ascii() {
        return false;
    }
    if !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    lower.ends_with("_index")
        || lower.ends_with("_at")
        || lower.ends_with("_focused")
        || matches!(
            lower.as_str(),
            "click"
                | "double_click"
                | "right_click"
                | "hover"
                | "drag_from_to"
                | "scroll"
                | "type_text"
                | "select"
                | "range_select"
                | "click_at"
                | "click_index"
                | "double_click_at"
                | "double_click_index"
                | "right_click_at"
                | "right_click_index"
                | "hover_at"
                | "hover_index"
                | "drag_from_to_at"
                | "drag_from_to_index"
                | "type_text_at"
                | "type_text_at_index"
                | "type_text_at_focused"
                | "scroll_at_index"
                | "select_index"
                | "select_at"
                | "range_select_index"
                | "range_select_at"
        )
}

fn explicit_operation_label(args: &Value) -> Option<String> {
    if let Some(m) = args
        .get("method")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(m.to_string());
    }
    if let Some(a) = args
        .get("action")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if looks_like_operation_name(a) {
            return Some(a.to_string());
        }
    }
    None
}

fn infer_mouse_operation(args: &Value) -> Result<String> {
    if args.get("from_index").is_some() && args.get("to_index").is_some() {
        return Ok("drag_from_to_index".to_string());
    }
    if has_index_targeting(args) {
        return Ok("click_index".to_string());
    }
    if has_xy(args) {
        if args.get("x2").is_some() && args.get("y2").is_some() {
            return Ok("drag_from_to_at".to_string());
        }
        return Ok("click_at".to_string());
    }
    Err(anyhow!(
        "Missing 'method' in tool_args (e.g. click_index, click_at). \
         'action' is the human target description, not the operation name."
    ))
}

fn infer_input_operation(args: &Value) -> Result<String> {
    if args.get("index").is_some() {
        if args.get("text").is_some() {
            return Ok("input_index".to_string());
        }
        return Ok("mouse_scroll_index".to_string());
    }
    if args.get("text").is_some() && has_xy(args) {
        return Ok("input_at".to_string());
    }
    if args.get("text").is_some() {
        return Ok("input_focused".to_string());
    }
    Err(anyhow!(
        "Missing targeting in tool_args (index, x+y, or focused text). \
         'action' is the human target description, not the operation name."
    ))
}

fn infer_modified_click_operation(args: &Value) -> Result<String> {
    if args.get("indices").is_some() || has_index_targeting(args) {
        let range = args
            .get("range_select")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        return Ok(if range {
            "range_select_index".to_string()
        } else {
            "select_index".to_string()
        });
    }
    if args.get("positions").is_some() || has_xy(args) {
        return Ok("select_at".to_string());
    }
    Err(anyhow!(
        "Missing 'method' in tool_args. \
         'action' is the human target description, not the operation name."
    ))
}

fn resolve_mouse_operation_label(args: &Value) -> Result<String> {
    explicit_operation_label(args)
        .ok_or_else(|| ())
        .or_else(|_| infer_mouse_operation(args))
}

fn resolve_input_operation_label(args: &Value) -> Result<String> {
    explicit_operation_label(args)
        .ok_or_else(|| ())
        .or_else(|_| infer_input_operation(args))
}

fn resolve_modified_click_operation_label(args: &Value) -> Result<String> {
    explicit_operation_label(args)
        .ok_or_else(|| ())
        .or_else(|_| infer_modified_click_operation(args))
}

/// Best-effort operation name for UI labels (qualified tool name + args).
pub fn operation_name_for_display(base: &str, raw_name: &str, args: &Value) -> String {
    if let Some((_, m)) = raw_name.split_once(':') {
        let m = m.trim();
        if !m.is_empty() {
            return m.to_string();
        }
    }
    if let Some(m) = explicit_operation_label(args) {
        return m;
    }
    match base {
        "mouse" => resolve_mouse_operation_label(args).unwrap_or_default(),
        "input" => resolve_input_operation_label(args).unwrap_or_default(),
        "modified_click" => resolve_modified_click_operation_label(args).unwrap_or_default(),
        _ => String::new(),
    }
}

pub fn route_mouse(args: &Value) -> Result<RoutedMouse> {
    let legacy = resolve_mouse_operation_label(args)?;
    let lower = legacy.to_ascii_lowercase();

    if lower.ends_with("_index") || has_index_targeting(args) {
        let method = match lower.as_str() {
            "drag_from_to_index" => "drag_from_to".to_string(),
            s if s.ends_with("_index") => strip_suffix(s, "_index"),
            "drag_from_to" => "drag_from_to".to_string(),
            other => other.to_string(),
        };
        return Ok(RoutedMouse {
            backend: MouseBackend::Index,
            method,
        });
    }

    if lower.ends_with("_at") || has_xy(args) {
        let method = match lower.as_str() {
            "drag_from_to_at" => "drag_from_to".to_string(),
            s if s.ends_with("_at") => strip_suffix(s, "_at"),
            other => other.to_string(),
        };
        return Ok(RoutedMouse {
            backend: MouseBackend::At,
            method,
        });
    }

    Err(anyhow!(
        "Cannot route mouse method '{legacy}': need index/from_index, or x+y."
    ))
}

pub fn route_input(args: &Value) -> Result<RoutedInput> {
    let label = resolve_input_operation_label(args)?;
    let lower = label.to_ascii_lowercase();

    if lower == "input_index" || lower.ends_with("_index") || args.get("index").is_some() {
        return Ok(RoutedInput {
            backend: InputBackend::Index,
            method: "input_index".to_string(),
        });
    }

    if lower == "input_focused"
        || lower.contains("focused")
        || (args.get("text").is_some() && !has_xy(args) && args.get("index").is_none())
    {
        return Ok(RoutedInput {
            backend: InputBackend::Focused,
            method: "input_focused".to_string(),
        });
    }

    if lower == "input_at" || lower.ends_with("_at") || has_xy(args) {
        return Ok(RoutedInput {
            backend: InputBackend::At,
            method: "input_at".to_string(),
        });
    }

    Err(anyhow!(
        "Cannot route input tool '{label}': need index, or x+y, or focused typing."
    ))
}

pub fn route_modified_click(args: &Value) -> Result<RoutedModifiedClick> {
    let legacy = resolve_modified_click_operation_label(args)?;
    let lower = legacy.to_ascii_lowercase();

    if has_index_targeting(args) || lower.ends_with("_index") {
        let method = match lower.as_str() {
            "range_select_index" | "range_select" => "range_select".to_string(),
            "select_index" | "select" => "select".to_string(),
            other => other.to_string(),
        };
        return Ok(RoutedModifiedClick {
            backend: ModifiedClickBackend::Index,
            method,
        });
    }

    if args.get("positions").is_some() || lower.ends_with("_at") || has_xy(args) {
        let method = match lower.as_str() {
            "range_select_at" | "range_select" => "range_select".to_string(),
            "select_at" | "select" => "select".to_string(),
            other => other.to_string(),
        };
        return Ok(RoutedModifiedClick {
            backend: ModifiedClickBackend::At,
            method,
        });
    }

    Err(anyhow!(
        "Cannot route modified_click method '{legacy}': need indices or positions/x+y."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn routes_mouse_click_at_with_legacy_action_operation() {
        let args = json!({
            "action": "click_at",
            "goal": "open wechat",
            "x": "215",
            "y": "920"
        });
        let r = route_mouse(&args).unwrap();
        assert_eq!(r.backend, MouseBackend::At);
        assert_eq!(r.method, "click");
    }

    #[test]
    fn routes_mouse_click_index() {
        let args = json!({ "method": "click_index", "goal": "g", "index": 2 });
        let r = route_mouse(&args).unwrap();
        assert_eq!(r.backend, MouseBackend::Index);
        assert_eq!(r.method, "click");
    }

    #[test]
    fn routes_mouse_click_index_when_action_is_human_description() {
        let args = json!({
            "goal": "打开微信应用",
            "action": "点击 Dock 栏中的微信图标",
            "index": 133
        });
        let r = route_mouse(&args).unwrap();
        assert_eq!(r.backend, MouseBackend::Index);
        assert_eq!(r.method, "click");
    }

    #[test]
    fn display_infers_click_index_from_index_only() {
        let args = json!({
            "goal": "g",
            "action": "点击 Dock 栏中的微信图标",
            "index": 133
        });
        assert_eq!(
            operation_name_for_display("mouse", "mouse", &args),
            "click_index"
        );
    }

    #[test]
    fn routes_input_index_from_index_and_text() {
        let args = json!({
            "goal": "type username",
            "action": "focus login field",
            "index": 5,
            "text": "alice"
        });
        let r = route_input(&args).unwrap();
        assert_eq!(r.backend, InputBackend::Index);
        assert_eq!(r.method, "input_index");
    }

    #[test]
    fn display_infers_input_index_from_index_only() {
        let args = json!({
            "goal": "g",
            "action": "type in search box",
            "index": 12,
            "text": "hello"
        });
        assert_eq!(
            operation_name_for_display("input", "input_index", &args),
            "input_index"
        );
    }
}
