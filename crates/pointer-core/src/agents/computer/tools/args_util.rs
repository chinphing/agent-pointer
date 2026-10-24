//! Shared argument parsing / validation aligned with PyProjects/pointer `vision_common` helpers.

use crate::agents::computer::input::actions::MouseButton;
use anyhow::{anyhow, Result};
use serde_json::Value;

pub const SCROLL_LINES_MIN: i32 = 1;
pub const SCROLL_LINES_MAX: i32 = 300;
pub const MOVE_OFFSET_MAX: i32 = 8000;

/// Clamp scroll `lines` to [1, 300] or [-300, -1] (Pointer `clamp_scroll_lines`).
pub fn clamp_scroll_lines(lines: i32) -> Result<i32> {
    if lines == 0 {
        return Err(anyhow!("lines cannot be 0"));
    }
    Ok(if lines > 0 {
        lines.clamp(SCROLL_LINES_MIN, SCROLL_LINES_MAX)
    } else {
        lines.clamp(-SCROLL_LINES_MAX, -SCROLL_LINES_MIN)
    })
}

/// Resolve `human_like` from tool args or session default (Python `_get_human_like`).
pub fn human_like_from_args(args: &Value, default: bool) -> bool {
    if args.get("human_like").is_some() {
        json_bool_loose(args.get("human_like"))
    } else {
        default
    }
}

/// App settings `computerHumanLike`.
pub fn effective_human_like_default() -> bool {
    crate::platform_config::effective_settings_global().computer_human_like
}

pub fn json_bool_loose(v: Option<&Value>) -> bool {
    match v {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => {
            matches!(s.to_lowercase().as_str(), "true" | "1" | "yes")
        }
        Some(Value::Number(n)) => n.as_i64() == Some(1),
        _ => false,
    }
}

/// Parse a required unsigned integer tool arg.
/// Accepts integer JSON numbers and numeric strings.
pub fn required_u32_arg(args: &Value, key: &str) -> Result<u32> {
    let Some(v) = args.get(key) else {
        return Err(anyhow!("Missing or invalid '{}' parameter", key));
    };
    value_to_u32_loose(v).ok_or_else(|| anyhow!("Missing or invalid '{}' parameter", key))
}

/// Parse a required float arg.
/// Accepts numeric JSON values and numeric strings.
pub fn required_f32_arg(args: &Value, key: &str) -> Result<f32> {
    let Some(v) = args.get(key) else {
        return Err(anyhow!("Missing or invalid '{}' parameter", key));
    };
    value_to_f32_loose(v).ok_or_else(|| anyhow!("Missing or invalid '{}' parameter", key))
}

/// Convert JSON value to u32, accepting integer-like strings.
pub fn value_to_u32_loose(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_i64().and_then(|i| u64::try_from(i).ok()))
            .map(|u| u as u32),
        Value::String(s) => s.trim().parse::<u32>().ok(),
        _ => None,
    }
}

/// Convert JSON value to f32, accepting numeric strings.
pub fn value_to_f32_loose(v: &Value) -> Option<f32> {
    match v {
        Value::Number(n) => n.as_f64().map(|f| f as f32),
        Value::String(s) => s.trim().parse::<f32>().ok(),
        _ => None,
    }
}

/// Parse overlay indices from JSON (array of ints, or comma-separated string, or JSON array string).
pub fn parse_indices(arg: Option<&Value>) -> Result<Vec<u32>> {
    let Some(v) = arg else {
        return Err(anyhow!(
            "Missing 'indices' (list of item numbers, e.g. [1,2,3] or \"1,2,3\")."
        ));
    };
    match v {
        Value::Array(a) => {
            let mut out = Vec::with_capacity(a.len());
            for x in a {
                let n = x
                    .as_u64()
                    .or_else(|| x.as_i64().and_then(|i| u64::try_from(i).ok()))
                    .ok_or_else(|| anyhow!("indices must be a list of integers"))?;
                out.push(n as u32);
            }
            Ok(out)
        }
        Value::String(s) => {
            let t = s.trim();
            if t.starts_with('[') {
                let arr: Vec<Value> = serde_json::from_str(t)
                    .map_err(|_| anyhow!("invalid JSON array in indices string"))?;
                return parse_indices(Some(&Value::Array(arr)));
            }
            if t.is_empty() {
                return Err(anyhow!("indices string is empty"));
            }
            let mut out = Vec::new();
            for part in t.split(',') {
                let n: u32 = part
                    .trim()
                    .parse()
                    .map_err(|_| anyhow!("invalid index in comma list: {:?}", part.trim()))?;
                out.push(n);
            }
            Ok(out)
        }
        _ => Err(anyhow!("indices must be an array or string")),
    }
}

/// Pixel offset for `move_offset` (`offset_x` / `offset_y` in tool_args).
pub fn move_offset_pixels(args: &Value) -> Result<(i32, i32)> {
    let ox = args
        .get("offset_x")
        .or_else(|| args.get("dx"))
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow!("Missing or invalid 'offset_x' parameter"))? as i32;
    let oy = args
        .get("offset_y")
        .or_else(|| args.get("dy"))
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow!("Missing or invalid 'offset_y' parameter"))? as i32;
    if ox.abs() > MOVE_OFFSET_MAX || oy.abs() > MOVE_OFFSET_MAX {
        return Err(anyhow!(
            "offset_x/offset_y must be within [-{0}, {0}]",
            MOVE_OFFSET_MAX
        ));
    }
    Ok((ox, oy))
}

pub fn require_non_empty_str(args: &Value, key: &str) -> Result<String> {
    let Some(v) = args.get(key) else {
        return Err(anyhow!("Missing or invalid '{}' parameter", key));
    };
    let s = match v {
        Value::String(s) => s.trim().to_string(),
        _ => {
            return Err(anyhow!("Missing or invalid '{}' parameter", key));
        }
    };
    if s.is_empty() {
        return Err(anyhow!("Missing required '{}' in tool_args.", key));
    }
    Ok(s)
}

/// `text` for type_* tools: JSON string, or integer/whole number (e.g. phone IDs the model emits as numbers).
pub fn text_from_args(v: Option<&Value>) -> Result<String> {
    let Some(v) = v else {
        return Err(anyhow!("Missing or invalid 'text' parameter"));
    };
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                return Ok(i.to_string());
            }
            if let Some(u) = n.as_u64() {
                return Ok(u.to_string());
            }
            let f = n
                .as_f64()
                .ok_or_else(|| anyhow!("Missing or invalid 'text' parameter"))?;
            if f.fract() == 0.0 && f.is_finite() {
                Ok(format!("{:.0}", f))
            } else {
                Ok(f.to_string())
            }
        }
        _ => Err(anyhow!("Missing or invalid 'text' parameter")),
    }
}

/// Wait seconds: Pointer allows 0..=60 (float).
pub fn parse_wait_seconds(arg: Option<&Value>) -> Result<f64> {
    let Some(v) = arg else {
        return Err(anyhow!("Missing 'seconds' in tool_args."));
    };
    let sec = match v {
        Value::Number(n) => n
            .as_f64()
            .ok_or_else(|| anyhow!("invalid seconds number"))?,
        Value::String(s) => s
            .trim()
            .parse::<f64>()
            .map_err(|_| anyhow!("invalid seconds string"))?,
        _ => return Err(anyhow!("seconds must be a number")),
    };
    if sec < 0.0 || sec > 60.0 {
        return Err(anyhow!("Seconds must be between 0 and 60."));
    }
    Ok(sec)
}

/// Mouse button from `button` (`left` / `middle` / `right`); defaults to left.
pub fn mouse_button_from_args(args: &Value) -> Result<MouseButton> {
    let Some(v) = args.get("button") else {
        return Ok(MouseButton::Left);
    };
    let Some(name) = v.as_str() else {
        return Err(anyhow!("button must be a string: left, middle, or right"));
    };
    MouseButton::parse(name).ok_or_else(|| {
        anyhow!(
            "Unknown button {:?}; use left, middle, or right",
            name.trim()
        )
    })
}

/// `clicks` for `CLICK`; defaults to 1. Only 1 and 2 are expressible.
pub fn click_count_from_args(args: &Value) -> Result<u32> {
    let Some(v) = args.get("clicks") else {
        return Ok(1);
    };
    let clicks = value_to_u32_loose(v)
        .ok_or_else(|| anyhow!("clicks must be an integer: 1 or 2"))?;
    if !(1..=2).contains(&clicks) {
        return Err(anyhow!("clicks must be 1 or 2, got {clicks}"));
    }
    Ok(clicks)
}

/// Wheel-notch scroll limit for one call.
///
/// `dx` / `dy` are **notch counts**, not pixels: the value goes straight to
/// `ComputerControl::scroll` / `scroll_horizontal`, which hands it to the OS
/// wheel primitive. One notch is whatever the platform and the app decide —
/// there is no pixel equivalent to promise, and none is claimed.
pub const SCROLL_NOTCHES_MAX: i32 = 100;

/// `dx` / `dy` for `SCROLL` in **wheel notches** (positive `dy` = up, `dx` = right).
///
/// At least one axis must be non-zero: there is no default direction to fall
/// back on, and guessing one would be worse than an error.
pub fn scroll_notches_from_args(args: &Value) -> Result<(i32, i32)> {
    fn axis(args: &Value, key: &str) -> Result<i32> {
        match args.get(key) {
            None => Ok(0),
            Some(v) => {
                let raw = value_to_f32_loose(v)
                    .ok_or_else(|| anyhow!("Missing or invalid '{}' parameter", key))?;
                Ok(raw.round() as i32)
            }
        }
    }
    let dx = axis(args, "dx")?;
    let dy = axis(args, "dy")?;
    if dx == 0 && dy == 0 {
        return Err(anyhow!(
            "scroll needs a non-zero dx or dy (wheel notches; positive dy = up, dx = right)"
        ));
    }
    Ok((
        dx.clamp(-SCROLL_NOTCHES_MAX, SCROLL_NOTCHES_MAX),
        dy.clamp(-SCROLL_NOTCHES_MAX, SCROLL_NOTCHES_MAX),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_scroll_rejects_zero() {
        assert!(clamp_scroll_lines(0).is_err());
    }

    #[test]
    fn human_like_from_args_uses_default_when_missing() {
        use serde_json::json;
        assert!(!human_like_from_args(&json!({"goal": "x"}), false));
        assert!(human_like_from_args(&json!({"goal": "x"}), true));
    }

    #[test]
    fn human_like_from_args_overrides_default() {
        use serde_json::json;
        assert!(human_like_from_args(&json!({"human_like": true}), false));
        assert!(!human_like_from_args(&json!({"human_like": false}), true));
    }

    #[test]
    fn clamp_scroll_clamps_magnitude() {
        assert_eq!(clamp_scroll_lines(500).unwrap(), 300);
        assert_eq!(clamp_scroll_lines(-500).unwrap(), -300);
        assert_eq!(clamp_scroll_lines(3).unwrap(), 3);
    }

    #[test]
    fn require_non_empty_str_reports_field_name_not_text() {
        use serde_json::json;
        let err = require_non_empty_str(&json!({ "index": 159 }), "goal")
            .unwrap_err()
            .to_string();
        assert!(err.contains("goal"));
        assert!(!err.contains("'text'"));
    }

    #[test]
    fn text_from_args_accepts_string_or_integer_number() {
        use serde_json::json;
        assert_eq!(
            text_from_args(Some(&json!("13856729034"))).unwrap(),
            "13856729034"
        );
        assert_eq!(
            text_from_args(Some(&json!(13856729034_u64))).unwrap(),
            "13856729034"
        );
        assert_eq!(text_from_args(Some(&json!(42))).unwrap(), "42");
        assert!(text_from_args(None).is_err());
        assert!(text_from_args(Some(&json!(true))).is_err());
    }

    #[test]
    fn required_u32_arg_accepts_numeric_string() {
        use serde_json::json;
        assert_eq!(
            required_u32_arg(&json!({"index": "150"}), "index").unwrap(),
            150
        );
        assert_eq!(
            required_u32_arg(&json!({"index": 150}), "index").unwrap(),
            150
        );
        assert!(required_u32_arg(&json!({"index": "x"}), "index").is_err());
    }

    #[test]
    fn required_f32_arg_accepts_numeric_string() {
        use serde_json::json;
        assert_eq!(required_f32_arg(&json!({"x": "450"}), "x").unwrap(), 450.0);
        assert_eq!(required_f32_arg(&json!({"x": 450}), "x").unwrap(), 450.0);
        assert!(required_f32_arg(&json!({"x": "x"}), "x").is_err());
    }

    #[test]
    fn mouse_button_defaults_to_left_and_parses_three_values() {
        use serde_json::json;
        assert_eq!(
            mouse_button_from_args(&json!({"x": 1})).unwrap(),
            MouseButton::Left
        );
        assert_eq!(
            mouse_button_from_args(&json!({"button": "middle"})).unwrap(),
            MouseButton::Middle
        );
        assert_eq!(
            mouse_button_from_args(&json!({"button": "RIGHT"})).unwrap(),
            MouseButton::Right
        );
        assert!(mouse_button_from_args(&json!({"button": "scroll"})).is_err());
        assert!(mouse_button_from_args(&json!({"button": 2})).is_err());
    }

    #[test]
    fn click_count_defaults_to_one_and_caps_at_two() {
        use serde_json::json;
        assert_eq!(click_count_from_args(&json!({})).unwrap(), 1);
        assert_eq!(click_count_from_args(&json!({"clicks": 2})).unwrap(), 2);
        assert_eq!(click_count_from_args(&json!({"clicks": "2"})).unwrap(), 2);
        assert!(click_count_from_args(&json!({"clicks": 0})).is_err());
        assert!(click_count_from_args(&json!({"clicks": 3})).is_err());
    }

    #[test]
    fn scroll_notches_reads_both_axes_and_rejects_noop() {
        use serde_json::json;
        assert_eq!(
            scroll_notches_from_args(&json!({"dx": 12, "dy": -34})).unwrap(),
            (12, -34)
        );
        assert_eq!(
            scroll_notches_from_args(&json!({"dy": "30"})).unwrap(),
            (0, 30)
        );
        assert!(scroll_notches_from_args(&json!({})).is_err());
        assert!(scroll_notches_from_args(&json!({"dx": 0, "dy": 0})).is_err());
        assert_eq!(
            scroll_notches_from_args(&json!({"dy": 999_999})).unwrap(),
            (0, SCROLL_NOTCHES_MAX)
        );
    }

    #[test]
    fn scroll_notches_are_not_rescaled() {
        use serde_json::json;
        // A notch is a notch: no divide-by-100, no rounding, no minimum of one.
        // Only the ±SCROLL_NOTCHES_MAX clamp touches the value.
        assert_eq!(scroll_notches_from_args(&json!({"dy": 1})).unwrap(), (0, 1));
        assert_eq!(scroll_notches_from_args(&json!({"dy": 7})).unwrap(), (0, 7));
        assert_eq!(scroll_notches_from_args(&json!({"dy": 100})).unwrap(), (0, 100));
    }
}
