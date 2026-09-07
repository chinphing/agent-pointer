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

pub fn parse_args(value: Value) -> Result<AskUserArgs> {
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
    fn ask_user_is_inheritable_to_subagent() {
        let reg = ToolRegistry::new();
        register_all(&reg);
        assert!(
            reg.is_inheritable_to_subagent("ask_user"),
            "ask_user must survive retain_inheritable_subagent_tools for self forks"
        );
    }
}
