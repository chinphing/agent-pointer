//! Interactive user-choice tool registration and argument validation.

use crate::tools::{parallel::ToolConflictClass, ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

const DOC_SOURCE: &str = "tools/prompts/ask_user.md";
const DOC: &str = include_str!("prompts/ask_user.md");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AskUserOption {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AskUserArgs {
    pub question: String,
    pub options: Vec<AskUserOption>,
    #[serde(default)]
    pub multi_select: bool,
}

/// Models sometimes stringify nested JSON (`options: "[{...}]"`). Coerce to an
/// array before typed deserialize. Trailing junk after a balanced `[...]` is ignored.
fn coerce_options_field(mut value: Value) -> Result<Value> {
    let Some(obj) = value.as_object_mut() else {
        return Ok(value);
    };
    let Some(raw) = obj.get("options").cloned() else {
        return Ok(value);
    };
    if raw.is_array() {
        return Ok(value);
    }
    let Some(s) = raw.as_str() else {
        return Err(anyhow!(
            "ask_user.options 必须是对象数组，不能是 {}",
            value_type_name(&raw)
        ));
    };
    let trimmed = s.trim();
    let parsed = if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        v
    } else if let Some(slice) = extract_balanced_json_array(trimmed) {
        serde_json::from_str::<Value>(slice)
            .map_err(|e| anyhow!("ask_user.options 字符串无法解析为 JSON 数组: {e}"))?
    } else {
        return Err(anyhow!(
            "ask_user.options 必须是对象数组；收到的是无法解析的字符串"
        ));
    };
    if !parsed.is_array() {
        return Err(anyhow!(
            "ask_user.options 必须是对象数组，字符串解析结果是 {}",
            value_type_name(&parsed)
        ));
    }
    log::info!(
        "ask_user: coerced stringified options to array (len={})",
        parsed.as_array().map(|a| a.len()).unwrap_or(0)
    );
    obj.insert("options".into(), parsed);
    Ok(value)
}

fn value_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// First balanced JSON array slice, respecting string escapes (tolerates trailing junk).
fn extract_balanced_json_array(s: &str) -> Option<&str> {
    let start = s.find('[')?;
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for i in start..bytes.len() {
        let c = bytes[i] as char;
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if c == '\\' {
                escape = true;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

pub fn parse_args(value: Value) -> Result<AskUserArgs> {
    let value = coerce_options_field(value)?;
    let mut args: AskUserArgs =
        serde_json::from_value(value).map_err(|e| anyhow!("ask_user 参数无效: {e}"))?;
    args.question = args.question.trim().to_string();
    if args.question.is_empty() {
        return Err(anyhow!("ask_user.question 不能为空"));
    }
    if !(2..=6).contains(&args.options.len()) {
        return Err(anyhow!("ask_user.options 必须包含 2 到 6 个选项"));
    }
    for option in &mut args.options {
        option.label = option.label.trim().to_string();
        if option.label.is_empty() {
            return Err(anyhow!("ask_user 选项 label 不能为空"));
        }
        option.description = option
            .description
            .take()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());
    }
    Ok(args)
}

pub fn register_all(reg: &ToolRegistry) {
    let handler: ToolHandler = Arc::new(|_args| {
        Err(anyhow!(
            "ask_user is executed by the chat runtime, not synchronous invoke"
        ))
    });
    let schema = json!({
        "type": "object",
        "properties": {
            "question": { "type": "string", "description": "The decision the user must make." },
            "options": {
                "type": "array",
                "description": "JSON array of option objects (not a stringified JSON array).",
                "minItems": 2,
                "maxItems": 6,
                "items": {
                    "type": "object",
                    "properties": {
                        "label": { "type": "string" },
                        "description": { "type": "string" }
                    },
                    "required": ["label"],
                    "additionalProperties": false
                }
            },
            "multi_select": { "type": "boolean", "default": false }
        },
        "required": ["question", "options"],
        "additionalProperties": false
    });
    reg.register(
        ToolEntry::new("ask_user", DOC_SOURCE, "low", false, DOC, handler)
            .with_schema(schema)
            .with_parallel_metadata(false, ToolConflictClass::SerialOnly),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_options() {
        let args = parse_args(json!({
            "question": "Choose a database",
            "options": [{"label": "SQLite"}, {"label": "Postgres", "description": "Shared server"}]
        }))
        .unwrap();
        assert_eq!(args.options.len(), 2);
        assert!(!args.multi_select);
    }

    #[test]
    fn rejects_too_few_options() {
        let err = parse_args(json!({
            "question": "Choose",
            "options": [{"label": "Only"}]
        }))
        .unwrap_err();
        assert!(err.to_string().contains("2 到 6"));
    }

    #[test]
    fn coerces_stringified_options_array() {
        let args = parse_args(json!({
            "question": "Write back remark?",
            "options": "\n[{\"label\": \"Save only\"}, {\"label\": \"Skip\"}, {\"label\": \"Fix todos first\"}]\n"
        }))
        .unwrap();
        assert_eq!(args.options.len(), 3);
        assert_eq!(args.options[0].label, "Save only");
    }

    #[test]
    fn coerces_stringified_options_with_trailing_junk() {
        // Models sometimes emit an extra trailing `]` after a valid array.
        let raw = concat!(
            r#"[{"label": "仅保存签字意见，不送审", "description": "停在送审前"},"#,
            r#"{"label": "暂不写回"},"#,
            r#"{"label": "先处理送审前待办"}]}"#
        );
        let args = parse_args(json!({
            "question": "是否写回？",
            "options": raw
        }))
        .unwrap();
        assert_eq!(args.options.len(), 3);
        assert_eq!(args.options[0].label, "仅保存签字意见，不送审");
    }

    #[test]
    fn ask_user_is_inheritable_to_subagent() {
        let reg = ToolRegistry::new();
        register_all(&reg);
        assert!(
            reg.is_inheritable_to_subagent("ask_user"),
            "ask_user must survive retain_inheritable_subagent_tools for self forks"
        );
    }
}
