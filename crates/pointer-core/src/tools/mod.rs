pub mod builtin;
pub mod file;
pub mod response;
pub mod skill;
pub mod task_board;
pub mod terminal;
pub mod tool_doc;
pub mod tool_md;

pub use tool_doc::{doc_markdown_without_schema_fence, json_schema_from_markdown, load_tool_doc_and_schema};

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

/// `file:write` / `file:edit` are high risk for UI; other `file` methods are low.
pub fn file_tool_effective_risk_level(raw_tool_name: &str, args: &Value) -> &'static str {
    let raw = normalize_tool_name_colons(raw_tool_name.trim());
    let method_from_qual = if let Some((base, method)) = raw.split_once(':') {
        if base.trim().eq_ignore_ascii_case("file") && !method.trim().is_empty() {
            Some(method.trim().to_ascii_lowercase())
        } else {
            None
        }
    } else {
        None
    };
    let m = method_from_qual
        .as_deref()
        .or_else(|| args.get("method").and_then(|v| v.as_str()).map(str::trim));
    match m {
        Some("write") | Some("edit") => "high",
        _ => "low",
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
    /// When true, the tool may only appear inside `<sidecar_tools>` / `<call>`, not as the root
    /// `<tool_name>` when any sidecar calls are present (see `response` tool docs).
    pub is_sidecar: bool,
    pub doc_markdown: String,
    pub prompt: Option<ToolPrompt>,
    pub handler: ToolHandler,
}

impl ToolEntry {
    pub fn new(
        name: impl Into<String>,
        risk_level: impl Into<String>,
        requires_approval: bool,
        doc_markdown: impl Into<String>,
        prompt: Option<ToolPrompt>,
        handler: ToolHandler,
    ) -> Self {
        Self::new_inner(
            name,
            risk_level,
            requires_approval,
            false,
            doc_markdown,
            prompt,
            handler,
        )
    }

    /// Sidecar-only tools (`task_board`, …): documented under **Sidecar tools** in system prompts.
    pub fn new_sidecar(
        name: impl Into<String>,
        risk_level: impl Into<String>,
        requires_approval: bool,
        doc_markdown: impl Into<String>,
        prompt: Option<ToolPrompt>,
        handler: ToolHandler,
    ) -> Self {
        Self::new_inner(
            name,
            risk_level,
            requires_approval,
            true,
            doc_markdown,
            prompt,
            handler,
        )
    }

    fn new_inner(
        name: impl Into<String>,
        risk_level: impl Into<String>,
        requires_approval: bool,
        is_sidecar: bool,
        doc_markdown: impl Into<String>,
        prompt: Option<ToolPrompt>,
        handler: ToolHandler,
    ) -> Self {
        let name = name.into();
        Self {
            def: ToolDef { name },
            risk_level: risk_level.into(),
            requires_approval,
            is_sidecar,
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

    /// Risk for a concrete invocation (`file` depends on `method` / qualified name).
    pub fn tool_risk_level_for_invocation(&self, raw_tool_name: &str, args: &Value) -> Option<String> {
        let base = registry_tool_base_name(raw_tool_name);
        if base == "file" {
            return Some(file_tool_effective_risk_level(raw_tool_name, args).to_string());
        }
        self.tool_risk_level(base)
    }

    pub fn tool_requires_approval(&self, name: &str) -> bool {
        self.inner
            .read()
            .get(name)
            .is_some_and(|e| e.requires_approval)
    }

    /// For merged `file` tool, only `write` and `edit` need approval; other tools use registry flag.
    pub fn tool_invocation_needs_approval(&self, tool_id: &str, args: &Value) -> bool {
        if tool_id == "file" {
            return matches!(
                args.get("method").and_then(|v| v.as_str()),
                Some("write") | Some("edit")
            );
        }
        self.tool_requires_approval(tool_id)
    }

    /// Whether the base registry name is registered as a sidecar-only tool.
    pub fn is_sidecar_tool(&self, raw_name: &str) -> bool {
        let base = registry_tool_base_name(raw_name);
        self.inner
            .read()
            .get(base)
            .is_some_and(|e| e.is_sidecar)
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
                        "parameters": {
                            "type": "object",
                            "properties": {}
                        }
                    }
                })
            })
            .collect()
    }

    pub fn prompt_context(&self, allow: &[String]) -> Vec<String> {
        let (regular, sidecar) = self.prompt_context_sections(allow);
        let mut out = Vec::new();
        if !regular.is_empty() {
            out.push(format!(
                "## Regular tools\n\n{}",
                regular.join("\n\n")
            ));
        }
        if let Some(s) = sidecar {
            out.push(s);
        }
        out
    }

    /// Split tool usage prompts: **Regular tools** vs **Sidecar tools** (English section titles).
    /// Returns `(regular_blocks, optional_sidecar_chapter)`; sidecar chapter omitted when empty.
    pub fn prompt_context_sections(&self, allow: &[String]) -> (Vec<String>, Option<String>) {
        let g = self.inner.read();
        let mut regular: Vec<(String, String)> = Vec::new();
        let mut sidecar: Vec<(String, String)> = Vec::new();
        for e in g.values() {
            if !allow.is_empty() && !allow.contains(&e.def.name) {
                continue;
            }
            let Some(p) = e.prompt.as_ref() else {
                continue;
            };
            let block = format!("[Tool usage: {}]\n{}", e.def.name, p.system_prompt);
            if e.is_sidecar {
                sidecar.push((e.def.name.clone(), block));
            } else {
                regular.push((e.def.name.clone(), block));
            }
        }
        drop(g);
        regular.sort_by(|a, b| a.0.cmp(&b.0));
        sidecar.sort_by(|a, b| a.0.cmp(&b.0));
        let regular_strs: Vec<String> = regular.into_iter().map(|(_, b)| b).collect();
        let sidecar_chapter = if sidecar.is_empty() {
            None
        } else {
            let intro = concat!(
                "These tools must **not** be used as the root `<tool_name>` when `<sidecar_tools>` is present.\n",
                "They may only appear inside `<sidecar_tools>` as one or more `<call>` entries.\n",
                "Each `<call>` uses the same `<tool_name>` / `<tool_args>` shape as a single tool invocation.\n",
                "Use **qualified** names **`tool:method`** in `<tool_name>` (e.g. **`task_board:patch`**) per the blocks below.\n",
            );
            let body: String = sidecar.into_iter().map(|(_, b)| b).collect::<Vec<_>>().join("\n\n");
            Some(format!("## Sidecar tools\n\n{intro}\n{body}"))
        };
        (regular_strs, sidecar_chapter)
    }
}

/// When the model emits multiple tool calls from one `<response>` (sidecar prefix + root tool),
/// every call except the **last** must be a registered **sidecar** tool; the last is the root primary.
pub fn validate_envelope_tool_batch(
    tools: &ToolRegistry,
    batch: &[crate::models::ToolCall],
) -> Result<(), String> {
    if batch.len() <= 1 {
        return Ok(());
    }
    for tc in &batch[..batch.len() - 1] {
        if !tools.is_sidecar_tool(&tc.name) {
            return Err(format!(
                "only sidecar tools may precede the root tool; got {}",
                tc.name
            ));
        }
    }
    let root = &batch[batch.len() - 1];
    if tools.is_sidecar_tool(&root.name) {
        return Err(format!(
            "root tool must not be a sidecar-only tool when multiple calls are present; got {}",
            root.name
        ));
    }
    Ok(())
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
    use super::file_tool_effective_risk_level;
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

    #[test]
    fn file_risk_high_only_write_edit() {
        assert_eq!(
            file_tool_effective_risk_level("file:write", &serde_json::json!({})),
            "high"
        );
        assert_eq!(
            file_tool_effective_risk_level("file:edit", &serde_json::json!({})),
            "high"
        );
        assert_eq!(
            file_tool_effective_risk_level("file", &serde_json::json!({"method": "read"})),
            "low"
        );
        assert_eq!(
            file_tool_effective_risk_level("file", &serde_json::json!({"method": "list"})),
            "low"
        );
        assert_eq!(
            file_tool_effective_risk_level("file:grep", &serde_json::json!({})),
            "low"
        );
    }
}

#[cfg(test)]
mod envelope_validation_tests {
    use super::validate_envelope_tool_batch;
    use super::ToolRegistry;
    use crate::models::ToolCall;
    use std::sync::Arc;

    fn reg() -> ToolRegistry {
        let r = ToolRegistry::new();
        let store = Arc::new(crate::tools::task_board::TaskBoardStore::default());
        crate::tools::builtin::register_all(&r, store);
        r
    }

    fn tc(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: name.into(),
            arguments: "{}".into(),
            status: "pending".into(),
            result: None,
            error: None,
            duration_ms: None,
            risk_level: None,
        }
    }

    #[test]
    fn batch_sidecar_then_terminal_ok() {
        let tools = reg();
        let batch = vec![tc("a", "task_board:patch"), tc("b", "terminal")];
        assert!(validate_envelope_tool_batch(&tools, &batch).is_ok());
    }

    #[test]
    fn batch_single_task_board_ok() {
        let tools = reg();
        let batch = vec![tc("a", "task_board:patch")];
        assert!(validate_envelope_tool_batch(&tools, &batch).is_ok());
    }

    #[test]
    fn batch_rejects_non_sidecar_prefix() {
        let tools = reg();
        let batch = vec![tc("a", "terminal"), tc("b", "file:read")];
        assert!(validate_envelope_tool_batch(&tools, &batch).is_err());
    }

    #[test]
    fn batch_rejects_sidecar_as_root_when_multiple() {
        let tools = reg();
        let batch = vec![tc("a", "task_board:patch"), tc("b", "task_board:replace")];
        assert!(validate_envelope_tool_batch(&tools, &batch).is_err());
    }
}
