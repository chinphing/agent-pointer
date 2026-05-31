//! Route unified computer tool `method` / `action` labels to internal backends.

use anyhow::{anyhow, Result};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseBackend {
    Index,
    At,
    Current,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositeBackend {
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

pub struct RoutedComposite {
    pub backend: CompositeBackend,
    pub method: String,
}

pub struct RoutedModifiedClick {
    pub backend: ModifiedClickBackend,
    pub method: String,
}

pub fn legacy_method_label(args: &Value) -> Result<String> {
    let m = args
        .get("method")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("action").and_then(|v| v.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Missing 'method' or 'action' in tool_args."))?;
    Ok(m.to_string())
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

pub fn route_mouse(args: &Value) -> Result<RoutedMouse> {
    let legacy = legacy_method_label(args)?;
    let lower = legacy.to_ascii_lowercase();

    if lower.ends_with("_current")
        || matches!(
            lower.as_str(),
            "click_current" | "double_click_current" | "right_click_current" | "scroll_at_current"
                | "move_offset"
        )
    {
        let method = match lower.as_str() {
            "scroll_at_current" => "scroll".to_string(),
            s if s.ends_with("_current") => strip_suffix(s, "_current"),
            other => other.to_string(),
        };
        return Ok(RoutedMouse {
            backend: MouseBackend::Current,
            method,
        });
    }

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
        "Cannot route mouse method '{legacy}': need index/from_index, or x+y, or a *_current method."
    ))
}

pub fn route_composite(args: &Value) -> Result<RoutedComposite> {
    let legacy = legacy_method_label(args)?;
    let lower = legacy.to_ascii_lowercase();

    if lower.ends_with("_index") || args.get("index").is_some() {
        let method = match lower.as_str() {
            "type_text_at_index" => "type_text".to_string(),
            "scroll_at_index" => "scroll".to_string(),
            s if s.ends_with("_index") => strip_suffix(s, "_index"),
            other => other.to_string(),
        };
        return Ok(RoutedComposite {
            backend: CompositeBackend::Index,
            method,
        });
    }

    if lower.contains("focused")
        || (args.get("text").is_some() && !has_xy(args) && args.get("index").is_none())
    {
        let method = match lower.as_str() {
            "type_text_at_focused" | "type_text_focused" => "type_text".to_string(),
            other => other.to_string(),
        };
        return Ok(RoutedComposite {
            backend: CompositeBackend::Focused,
            method,
        });
    }

    if lower.ends_with("_at") || has_xy(args) {
        let method = match lower.as_str() {
            "type_text_at" => "type_text".to_string(),
            s if s.ends_with("_at") => strip_suffix(s, "_at"),
            other => other.to_string(),
        };
        return Ok(RoutedComposite {
            backend: CompositeBackend::At,
            method,
        });
    }

    Err(anyhow!(
        "Cannot route composite_action method '{legacy}': need index, or x+y, or focused typing."
    ))
}

pub fn route_modified_click(args: &Value) -> Result<RoutedModifiedClick> {
    let legacy = legacy_method_label(args)?;
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
    fn routes_mouse_click_at_with_action_field() {
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
}
