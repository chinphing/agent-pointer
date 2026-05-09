pub mod builtin;
pub mod general;
pub mod math;
pub mod skills;
pub mod terminal;
pub mod text;
pub mod workspace;

use crate::models::ToolDef;
use anyhow::Result;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

pub type ToolHandler = Arc<dyn Fn(serde_json::Value) -> Result<String> + Send + Sync>;

/// Parse model/provider `function.arguments` into a JSON value for tool handlers.
///
/// Some APIs return arguments as a JSON-encoded string (double encoding), or wrap the
/// payload in `arguments` / `params`. Markdown code fences around JSON also appear in the wild.
pub fn parse_tool_call_arguments(raw: &str) -> serde_json::Value {
    use serde_json::Value;

    let cleaned = strip_optional_code_fence(raw);
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return Value::Null;
    }

    let mut v: Value = match serde_json::from_str(trimmed) {
        Ok(x) => x,
        Err(_) => return Value::Null,
    };

    for _ in 0..3 {
        match &v {
            Value::String(s) => {
                let t = s.trim();
                if t.is_empty() {
                    return Value::Null;
                }
                match serde_json::from_str(t) {
                    Ok(next) => v = next,
                    Err(_) => break,
                }
            }
            _ => break,
        }
    }

    if let Value::Object(map) = &v {
        for key in ["arguments", "params", "parameters", "input"] {
            if let Some(inner) = map.get(key) {
                if inner.is_object() {
                    return inner.clone();
                }
                if let Value::String(s) = inner {
                    let nested = parse_tool_call_arguments(s);
                    if nested.is_object() {
                        return nested;
                    }
                }
            }
        }
    }

    v
}

fn strip_optional_code_fence(s: &str) -> String {
    let s = s.trim();
    if !s.starts_with("```") {
        return s.to_string();
    }
    let mut lines: Vec<&str> = s.lines().collect();
    if lines.first().is_some_and(|l| l.trim_start().starts_with('`')) {
        lines.remove(0);
    }
    while let Some(last) = lines.last() {
        let t = last.trim();
        if t == "```" || t.is_empty() {
            lines.pop();
        } else {
            break;
        }
    }
    lines.join("\n").trim().to_string()
}

#[derive(Debug, Clone)]
pub struct ToolPrompt {
    pub system_prompt: String,
}

pub struct ToolEntry {
    pub def: ToolDef,
    pub prompt: Option<ToolPrompt>,
    pub handler: ToolHandler,
}

#[derive(Default)]
pub struct ToolRegistry {
    inner: RwLock<HashMap<String, ToolEntry>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, def: ToolDef, handler: ToolHandler) {
        self.register_with_prompt(def, None, handler);
    }

    pub fn register_with_prompt(
        &self,
        def: ToolDef,
        prompt: Option<ToolPrompt>,
        handler: ToolHandler,
    ) {
        self.inner.write().insert(
            def.name.clone(),
            ToolEntry {
                def,
                prompt,
                handler,
            },
        );
    }

    pub fn list_defs(&self) -> Vec<ToolDef> {
        self.inner.read().values().map(|e| e.def.clone()).collect()
    }

    pub fn get_def(&self, name: &str) -> Option<ToolDef> {
        self.inner.read().get(name).map(|e| e.def.clone())
    }

    pub fn invoke(&self, name: &str, args: serde_json::Value) -> Result<String> {
        let g = self.inner.read();
        let entry = g
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("未注册的工具: {name}"))?;
        let handler = entry.handler.clone();
        drop(g);
        handler(args)
    }

    pub fn openai_tools(&self, allow: &[String]) -> Vec<serde_json::Value> {
        self.inner
            .read()
            .values()
            .filter(|e| allow.is_empty() || allow.contains(&e.def.name))
            .map(|e| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": e.def.name,
                        "description": e.def.description,
                        "parameters": e.def.parameters_schema
                    }
                })
            })
            .collect()
    }

    pub fn prompt_context(&self, allow: &[String]) -> Vec<String> {
        self.inner
            .read()
            .values()
            .filter(|e| allow.is_empty() || allow.contains(&e.def.name))
            .filter_map(|e| {
                e.prompt
                    .as_ref()
                    .map(|p| format!("【工具使用说明：{}】\n{}", e.def.name, p.system_prompt))
            })
            .collect()
    }
}

#[cfg(test)]
mod parse_args_tests {
    use super::parse_tool_call_arguments;

    #[test]
    fn unwraps_json_string_payload() {
        let inner = r#"{"path":"crates/x.rs","content":"fn main(){}"}"#;
        let double = serde_json::to_string(inner).unwrap();
        let v = parse_tool_call_arguments(&double);
        assert_eq!(v["path"], "crates/x.rs");
        assert!(v["content"].as_str().is_some());
    }

    #[test]
    fn unwraps_arguments_wrapper() {
        let raw = r#"{"arguments":{"path":"a.txt","content":"z"}}"#;
        let v = parse_tool_call_arguments(raw);
        assert_eq!(v["path"], "a.txt");
    }

    #[test]
    fn strips_markdown_fence() {
        let raw = "```json\n{\"path\":\"b\",\"content\":\"c\"}\n```";
        let v = parse_tool_call_arguments(raw);
        assert_eq!(v["path"], "b");
    }
}
