pub mod builtin;
pub mod display;
pub mod file;
pub mod response;
pub mod run_subagent;
pub mod skill;
pub mod terminal;
pub mod web_search;
pub mod tool_doc;
pub mod tool_md;

pub use display::{default_display, format_tool_display, ToolDisplay, ToolDisplayFn};

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

/// Whether `registry_tool_id` is permitted by an allow list (exact base or `base:method` entry).
pub fn registry_tool_in_allow_list(allowed: &[String], registry_tool_id: &str) -> bool {
    if allowed.is_empty() {
        return true;
    }
    allowed
        .iter()
        .any(|a| registry_tool_base_name(a) == registry_tool_id)
}

/// Map retired split computer tool ids to unified family names for allow lists.
pub fn remap_split_computer_tool_allow_names(names: &mut Vec<String>) {
    const TO_FAMILY: &[(&str, &str)] = &[
        ("mouse_index", "mouse"),
        ("mouse_at", "mouse"),
        ("mouse_current", "mouse"),
        ("composite_action_index", "composite_action"),
        ("composite_action_at", "composite_action"),
        ("composite_action_focused", "composite_action"),
        ("modified_click_index", "modified_click"),
        ("modified_click_at", "modified_click"),
    ];
    for &(from, to) in TO_FAMILY {
        if names
            .iter()
            .any(|n| registry_tool_base_name(n) == from)
            && !names.iter().any(|n| registry_tool_base_name(n) == to)
        {
            names.push(to.to_string());
        }
    }
    names.retain(|n| {
        let base = registry_tool_base_name(n);
        !matches!(
            base,
            "mouse_index"
                | "mouse_at"
                | "mouse_current"
                | "composite_action_index"
                | "composite_action_at"
                | "composite_action_focused"
                | "modified_click_index"
                | "modified_click_at"
        )
    });
}

/// Keep allow-list entries whose registry base exists; inject base names for `tool:method` entries.
pub fn normalize_allowed_tool_names(names: &mut Vec<String>, available: &std::collections::HashSet<String>) {
    remap_split_computer_tool_allow_names(names);
    names.retain(|name| available.contains(registry_tool_base_name(name)));
    let extras: Vec<String> = names
        .iter()
        .map(|n| registry_tool_base_name(n).to_string())
        .filter(|base| available.contains(base.as_str()) && !names.contains(base))
        .collect();
    names.extend(extras);
    names.sort();
    names.dedup();
}

/// Registry id: strip optional `:method` suffix (`mouse:click_index` → `mouse`, `wait` → `wait`).
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
    pub handler: ToolHandler,
    /// Optional UI label/summary formatter for chat tool cards.
    pub display: Option<ToolDisplayFn>,
}

impl ToolEntry {
    pub fn new(
        name: impl Into<String>,
        risk_level: impl Into<String>,
        requires_approval: bool,
        doc_markdown: impl Into<String>,
        handler: ToolHandler,
    ) -> Self {
        Self::new_inner(
            name,
            risk_level,
            requires_approval,
            false,
            doc_markdown,
            handler,
        )
    }

    /// Sidecar-only tools (`task_board`, …): enforced by [`ToolRegistry::is_sidecar_tool`] / envelope validation; long-form docs live in `doc_markdown` (`generate_tools_system_appendix`).
    pub fn new_sidecar(
        name: impl Into<String>,
        risk_level: impl Into<String>,
        requires_approval: bool,
        doc_markdown: impl Into<String>,
        handler: ToolHandler,
    ) -> Self {
        Self::new_inner(
            name,
            risk_level,
            requires_approval,
            true,
            doc_markdown,
            handler,
        )
    }

    fn new_inner(
        name: impl Into<String>,
        risk_level: impl Into<String>,
        requires_approval: bool,
        is_sidecar: bool,
        doc_markdown: impl Into<String>,
        handler: ToolHandler,
    ) -> Self {
        let name = name.into();
        Self {
            def: ToolDef { name },
            risk_level: risk_level.into(),
            requires_approval,
            is_sidecar,
            doc_markdown: doc_markdown.into(),
            handler,
            display: None,
        }
    }

    pub fn with_display(mut self, display: ToolDisplayFn) -> Self {
        self.display = Some(display);
        self
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

    /// UI display label + parameter summary for a tool invocation (not sent to the LLM).
    pub fn format_display(&self, raw_name: &str, args: &serde_json::Value) -> ToolDisplay {
        let base = registry_tool_base_name(raw_name);
        let custom = self.inner.read().get(base).and_then(|e| e.display.clone());
        format_tool_display(raw_name, args, custom.as_ref())
    }

    pub fn openai_tools(&self, allow: &[String]) -> Vec<serde_json::Value> {
        let mut out: Vec<serde_json::Value> = Vec::new();
        for e in self.inner.read().values() {
            if !registry_tool_allowed(&e.def.name, allow) {
                continue;
            }
            out.push(openai_tool_entry(
                &e.def.name,
                &openai_description_from_doc(&e.doc_markdown),
                openai_parameters_from_doc_or_builtin(&e.def.name, &e.doc_markdown),
            ));
        }
        out.sort_by(|a, b| {
            a["function"]["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["function"]["name"].as_str().unwrap_or(""))
        });
        out
    }
}

/// Whether a registry base tool (or any of its qualified variants) is allowed.
fn registry_tool_allowed(base: &str, allow: &[String]) -> bool {
    if allow.is_empty() {
        return true;
    }
    if allow.iter().any(|a| a == base) {
        return true;
    }
    let prefix = format!("{base}:");
    allow.iter().any(|a| a.starts_with(&prefix))
}

fn openai_tool_entry(name: &str, description: &str, parameters: Value) -> Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    })
}

/// Validate multi-call batch semantics:
/// - sidecar tools may appear multiple times and in any order,
/// - non-sidecar ("primary") tools may appear at most once per batch.
pub fn validate_envelope_tool_batch(
    tools: &ToolRegistry,
    batch: &[crate::models::ToolCall],
) -> Result<(), String> {
    let primary: Vec<&str> = batch
        .iter()
        .filter(|tc| !tools.is_sidecar_tool(&tc.name))
        .map(|tc| tc.name.as_str())
        .collect();
    if primary.len() > 1 {
        return Err(format!(
            "at most one non-sidecar tool is allowed per batch; got {}",
            primary.join(", ")
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

fn openai_parameters_from_doc_or_builtin(name: &str, doc: &str) -> serde_json::Value {
    match json_schema_from_markdown(doc) {
        Ok(schema) if schema.is_object() => schema,
        _ => panic!("Tool `{name}` missing valid schema in doc_markdown frontmatter"),
    }
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
        assert_eq!(registry_tool_base_name("file:read"), "file");
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
mod openai_tools_schema_tests {
    use super::ToolEntry;
    use super::ToolRegistry;
    use std::sync::Arc;

    #[test]
    fn openai_tools_file_uses_name_and_method() {
        use super::ToolEntry;
        use super::ToolRegistry;
        use std::sync::Arc;

        let reg = ToolRegistry::new();
        reg.register(ToolEntry::new(
            "file",
            "low",
            false,
            "### `file`\nUnified workspace file tools.",
            Arc::new(|_| Ok(String::new())),
        ));

        let tools = reg.openai_tools(&["file".into()]);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "file");
        let params = &tools[0]["function"]["parameters"];
        assert_eq!(params["required"][0], "method");
        assert!(params["properties"]["method"]["enum"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == "read")));
    }

    #[test]
    fn openai_tools_task_board_uses_name_and_method() {
        let reg = ToolRegistry::new();
        reg.register(ToolEntry::new_sidecar(
            "task_board",
            "low",
            false,
            "### `task_board`\nSession task board.",
            Arc::new(|_| Ok(String::new())),
        ));

        let tools = reg.openai_tools(&["task_board:patch".into()]);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "task_board");
        assert_eq!(tools[0]["function"]["parameters"]["required"][0], "method");
    }

    #[test]
    fn openai_tools_skill_uses_name_and_method() {
        let reg = ToolRegistry::new();
        reg.register(ToolEntry::new(
            "skill",
            "low",
            false,
            "### `skill`\nSkill loader.",
            Arc::new(|_| Ok(String::new())),
        ));

        let tools = reg.openai_tools(&["skill".into()]);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "skill");
        assert_eq!(tools[0]["function"]["parameters"]["required"][0], "method");
    }

    #[test]
    fn normalize_allowed_injects_base_for_qualified_entry() {
        use super::normalize_allowed_tool_names;
        use std::collections::HashSet;

        let available: HashSet<String> = ["task_board".into(), "file".into()].into_iter().collect();
        let mut names = vec!["task_board:patch".into(), "file:read".into()];
        normalize_allowed_tool_names(&mut names, &available);
        assert!(names.contains(&"task_board".to_string()));
        assert!(names.contains(&"file".to_string()));
    }

    #[test]
    fn registry_tool_in_allow_list_accepts_qualified_entry() {
        use super::registry_tool_in_allow_list;
        let allow = vec!["task_board:patch".into()];
        assert!(registry_tool_in_allow_list(&allow, "task_board"));
        assert!(!registry_tool_in_allow_list(&allow, "terminal"));
    }

    #[test]
    fn openai_tools_uses_builtin_schema_when_doc_has_no_json_fence() {
        let reg = ToolRegistry::new();
        reg.register(ToolEntry::new(
            "captcha_verify",
            "high",
            false,
            "plain doc without schema fence",
            Arc::new(|_| Ok(String::new())),
        ));

        let tools = reg.openai_tools(&[]);
        assert_eq!(tools.len(), 1);
        let params = &tools[0]["function"]["parameters"];
        assert_eq!(params["required"][0], "action");
        assert_eq!(params["required"][2], "index_captcha_area");
        assert_eq!(params["properties"]["index_captcha_area"]["type"], "integer");
        assert_eq!(params["properties"]["is_slider"]["type"], "boolean");
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
        let store = Arc::new(crate::task_board::TaskBoardStore::new());
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
            display_label: None,
            display_summary: None,
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
    fn batch_all_sidecars_ok() {
        let tools = reg();
        let batch = vec![tc("a", "task_board:patch"), tc("b", "task_board:replace")];
        assert!(validate_envelope_tool_batch(&tools, &batch).is_ok());
    }

    #[test]
    fn batch_allows_sidecar_calls_around_single_primary() {
        let tools = reg();
        let batch = vec![
            tc("a", "task_board:patch"),
            tc("b", "terminal"),
            tc("c", "task_board:replace"),
        ];
        assert!(validate_envelope_tool_batch(&tools, &batch).is_ok());
    }

    #[test]
    fn batch_rejects_more_than_one_primary_tool() {
        let tools = reg();
        let batch = vec![tc("a", "terminal"), tc("b", "file:read"), tc("c", "task_board:patch")];
        assert!(validate_envelope_tool_batch(&tools, &batch).is_err());
    }
}
