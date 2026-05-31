//! Shared argument parsing / validation aligned with PyProjects/pointer `vision_common` helpers.

use crate::agents::computer::vision::vision_state::{CornerAnchor, VisionState};
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

/// Resolve overlay index (+ optional anchor/dx/dy) to screen pixels.
pub fn resolve_index_pixels(
    vision: &VisionState,
    args: &Value,
    index: u32,
) -> Result<(i32, i32)> {
    let out = if let Some(anchor) = args
        .get("anchor")
        .and_then(|v| v.as_str())
        .and_then(CornerAnchor::parse)
    {
        let dx = args.get("dx").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let dy = args.get("dy").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        vision.resolve_index_anchor(index, anchor, dx, dy)
    } else {
        vision.resolve_index(index)
    };
    out.ok_or_else(|| anyhow!("Index {} not found in current annotation", index))
}

pub fn require_non_empty_str(args: &Value, key: &str) -> Result<String> {
    let s = text_from_args(args.get(key))?
        .trim()
        .to_string();
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
        assert_eq!(required_u32_arg(&json!({"index": "150"}), "index").unwrap(), 150);
        assert_eq!(required_u32_arg(&json!({"index": 150}), "index").unwrap(), 150);
        assert!(required_u32_arg(&json!({"index": "x"}), "index").is_err());
    }

    #[test]
    fn required_f32_arg_accepts_numeric_string() {
        use serde_json::json;
        assert_eq!(required_f32_arg(&json!({"x": "450"}), "x").unwrap(), 450.0);
        assert_eq!(required_f32_arg(&json!({"x": 450}), "x").unwrap(), 450.0);
        assert!(required_f32_arg(&json!({"x": "x"}), "x").is_err());
    }
}
