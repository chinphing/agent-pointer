pub mod builtin;
pub mod general;
pub mod math;
pub mod response;
pub mod skills;
pub mod terminal;
pub mod text;
pub mod tool_md;
pub mod workspace;

use crate::models::ToolDef;
use anyhow::Result;
use parking_lot::RwLock;
use serde_json::Value;
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

/// Normalize `tool：method` (fullwidth colon U+FF1A) to ASCII `:` so qualified names match the registry.
fn normalize_tool_name_colons(s: &str) -> String {
    s.chars()
        .map(|c| if c == '：' { ':' } else { c })
        .collect()
}

/// Registry id: strip the optional `:method` suffix (`mouse:click_index` → `mouse`, `wait` → `wait`).
pub fn registry_tool_base_name(raw: &str) -> &str {
    match raw.trim().split_once(':') {
        Some((base, rest)) if !base.is_empty() && !rest.trim().is_empty() => base.trim(),
        _ => raw.trim(),
    }
}

/// If `raw_name` is `tool:method`, return `(tool, args)` and ensure `args["method"]` is set when missing.
pub fn merge_tool_method_from_qualified_name(raw_name: &str, mut args: Value) -> (String, Value) {
    let raw_name = normalize_tool_name_colons(raw_name.trim());
    if raw_name.is_empty() {
        return (String::new(), args);
    }
    let Some((base, method)) = raw_name.split_once(':') else {
        return (raw_name, args);
    };
    let base = base.trim();
    let method = method.trim();
    if base.is_empty() || method.is_empty() {
        return (raw_name, args);
    }
    if let Value::Object(ref mut map) = args {
        map.entry("method".to_string())
            .or_insert_with(|| Value::String(method.to_string()));
    }
    (base.to_string(), args)
}

#[derive(Debug, Clone)]
pub struct ToolPrompt {
    pub system_prompt: String,
}

/// One registered tool: identity ([`ToolDef`]), OpenAI/XML documentation, approval policy, handler.
#[derive(Clone)]
pub struct ToolEntry {
    pub def: ToolDef,
    pub risk_level: String,
    pub requires_approval: bool,
    pub parameters_schema: Value,
    pub doc_markdown: String,
    pub prompt: Option<ToolPrompt>,
    pub handler: ToolHandler,
}

impl ToolEntry {
    pub fn new(
        name: impl Into<String>,
        risk_level: impl Into<String>,
        requires_approval: bool,
        parameters_schema: Value,
        doc_markdown: impl Into<String>,
        prompt: Option<ToolPrompt>,
        handler: ToolHandler,
    ) -> Self {
        let name = name.into();
        Self {
            def: ToolDef { name },
            risk_level: risk_level.into(),
            requires_approval,
            parameters_schema,
            doc_markdown: doc_markdown.into(),
            prompt,
            handler,
        }
    }
}

#[derive(Debug, Clone)]
pub struct XmlToolDescriptor {
    pub name: String,
    pub doc_markdown: String,
    pub parameters_schema: Value,
}

#[derive(Default)]
pub struct ToolRegistry {
    inner: RwLock<HashMap<String, ToolEntry>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, entry: ToolEntry) {
        self.inner
            .write()
            .insert(entry.def.name.clone(), entry);
    }

    pub fn list_defs(&self) -> Vec<ToolDef> {
        self.inner.read().values().map(|e| e.def.clone()).collect()
    }

    pub fn get_def(&self, name: &str) -> Option<ToolDef> {
        self.inner.read().get(name).map(|e| e.def.clone())
    }

    pub fn tool_risk_level(&self, name: &str) -> Option<String> {
        self.inner
            .read()
            .get(name)
            .map(|e| e.risk_level.clone())
    }

    pub fn tool_requires_approval(&self, name: &str) -> bool {
        self.inner
            .read()
            .get(name)
            .is_some_and(|e| e.requires_approval)
    }

    pub fn xml_tool_descriptors(&self, allow: &[String]) -> Vec<XmlToolDescriptor> {
        let mut out: Vec<XmlToolDescriptor> = self
            .inner
            .read()
            .values()
            .filter(|e| allow.is_empty() || allow.contains(&e.def.name))
            .map(|e| XmlToolDescriptor {
                name: e.def.name.clone(),
                doc_markdown: e.doc_markdown.clone(),
                parameters_schema: e.parameters_schema.clone(),
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
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
                let description = openai_description_from_doc(&e.doc_markdown);
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": e.def.name,
                        "description": description,
                        "parameters": e.parameters_schema
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
                    .map(|p| format!("[Tool usage: {}]\n{}", e.def.name, p.system_prompt))
            })
            .collect()
    }
}

fn openai_description_from_doc(doc: &str) -> String {
    let t = doc.trim();
    if t.is_empty() {
        return String::new();
    }
    const MAX: usize = 1024;
    let mut s: String = t.chars().take(MAX).collect();
    if t.chars().count() > MAX {
        s.push_str("…");
    }
    s
}

#[cfg(test)]
mod parse_args_tests {
    use super::merge_tool_method_from_qualified_name;
    use super::parse_tool_call_arguments;
    use super::registry_tool_base_name;

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

    #[test]
    fn registry_tool_base_name_splits_method_suffix() {
        assert_eq!(registry_tool_base_name("mouse:click_index"), "mouse");
        assert_eq!(registry_tool_base_name("wait"), "wait");
        assert_eq!(registry_tool_base_name("response"), "response");
    }

    #[test]
    fn merge_tool_method_inserts_method_when_missing() {
        let args = serde_json::json!({"goal": "g", "action": "a", "index": 3});
        let (id, out) = merge_tool_method_from_qualified_name("mouse:click_index", args);
        assert_eq!(id, "mouse");
        assert_eq!(out["method"], "click_index");
    }

    #[test]
    fn merge_tool_method_keeps_existing_method() {
        let args = serde_json::json!({"method": "click_at", "x": 1});
        let (id, out) = merge_tool_method_from_qualified_name("mouse:click_index", args);
        assert_eq!(id, "mouse");
        assert_eq!(out["method"], "click_at");
    }

    #[test]
    fn merge_tool_method_fullwidth_colon() {
        let args = serde_json::json!({"goal": "g", "index": 1});
        let (id, out) = merge_tool_method_from_qualified_name("mouse：click_index", args);
        assert_eq!(id, "mouse");
        assert_eq!(out["method"], "click_index");
    }
}
