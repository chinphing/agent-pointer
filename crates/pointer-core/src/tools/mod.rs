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

use crate::agents::computer::tool_names::{ACTION_VERIFY, ACTION_VERIFY_LEGACY_UNDERSCORE};
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

fn tool_matches_allow_entry(registry_tool_id: &str, allow_entry: &str) -> bool {
    let allow = allow_entry.trim();
    if registry_tool_id == allow {
        return true;
    }
    if let Some(fam) =
        crate::agents::computer::input::timing::desktop_tool_family_id(registry_tool_id)
    {
        if fam == allow {
            return true;
        }
    }
    if allow == "task_board" && registry_tool_id.starts_with("task_board_") {
        return true;
    }
    if allow == "file" && registry_tool_id.starts_with("file_") {
        return true;
    }
    if allow == "skill" && registry_tool_id.starts_with("skill_") {
        return true;
    }
    false
}

/// Whether `registry_tool_id` is permitted by an allow list (exact id or family name).
pub fn registry_tool_in_allow_list(allowed: &[String], registry_tool_id: &str) -> bool {
    if allowed.is_empty() {
        return true;
    }
    allowed
        .iter()
        .any(|a| tool_matches_allow_entry(registry_tool_id, a))
}

/// Expand family allow entries (e.g. `mouse`, `captcha_verify`) into flat registry tool ids.
pub fn expand_family_allow_names(
    names: &[String],
    available: &std::collections::HashSet<String>,
) -> Vec<String> {
    if names.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for allow_entry in names {
        let allow = allow_entry.trim();
        if available.contains(allow) {
            out.push(allow.to_string());
        }
        for reg in available {
            if tool_matches_allow_entry(reg, allow) {
                out.push(reg.clone());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Map retired split computer tool ids to unified family names for allow lists.
pub fn remap_split_computer_tool_allow_names(names: &mut Vec<String>) {
    const TO_FAMILY: &[(&str, &str)] = &[
        (ACTION_VERIFY_LEGACY_UNDERSCORE, ACTION_VERIFY),
        ("mouse_index", "mouse"),
        ("mouse_at", "mouse"),
        ("mouse_current", "mouse"),
        ("input_index", "input"),
        ("input_at", "input"),
        ("input_focused", "input"),
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
            ACTION_VERIFY_LEGACY_UNDERSCORE
                | "mouse_index"
                | "mouse_at"
                | "mouse_current"
                | "input_index"
                | "input_at"
                | "input_focused"
                | "modified_click_index"
                | "modified_click_at"
        )
    });
}

/// Keep allow-list entries that exist in the registry; inject exact registry ids when needed.
pub fn normalize_allowed_tool_names(names: &mut Vec<String>, available: &std::collections::HashSet<String>) {
    remap_split_computer_tool_allow_names(names);
    names.retain(|name| {
        let base = registry_tool_base_name(name);
        available.contains(base)
            || available
                .iter()
                .any(|reg| tool_matches_allow_entry(reg, base))
    });
    let extras: Vec<String> = names
        .iter()
        .map(|n| registry_tool_base_name(n).to_string())
        .filter(|base| available.contains(base.as_str()) && !names.contains(base))
        .collect();
    names.extend(extras);
    names.sort();
    names.dedup();
}

/// Registry tool id (trimmed); must match the flat name registered in [`ToolRegistry`].
pub fn registry_tool_base_name(raw: &str) -> &str {
    raw.trim()
}

/// Flat `file_write` / `file_edit` need approval; other file tools are low risk.
pub fn file_tool_effective_risk_level(tool_name: &str, _args: &Value) -> &'static str {
    match tool_name.trim() {
        "file_write" | "file_edit" => "high",
        _ => "low",
    }
}

/// Normalize tool id from a provider tool call (flat registry name only).
pub fn normalize_tool_invoke_name(raw_name: &str, args: Value) -> (String, Value) {
    (raw_name.trim().to_string(), args)
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
    /// Repo-relative path under `crates/pointer-core/src/` to the tool prompt `.md`.
    /// Dedup key for [`crate::tools_system_appendix::generate_tools_system_appendix`].
    pub doc_source: &'static str,
    pub doc_markdown: String,
    /// Standalone JSON Schema; when present, used instead of extracting from `doc_markdown`
    /// YAML frontmatter. Set when tools are registered from `.schema.yaml` files.
    pub schema: Option<serde_json::Value>,
    pub handler: ToolHandler,
    /// Optional UI label/summary formatter for chat tool cards.
    pub display: Option<ToolDisplayFn>,
}

impl ToolEntry {
    pub fn new(
        name: impl Into<String>,
        doc_source: &'static str,
        risk_level: impl Into<String>,
        requires_approval: bool,
        doc_markdown: impl Into<String>,
        handler: ToolHandler,
    ) -> Self {
        Self::new_inner(
            name,
            doc_source,
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
        doc_source: &'static str,
        risk_level: impl Into<String>,
        requires_approval: bool,
        doc_markdown: impl Into<String>,
        handler: ToolHandler,
    ) -> Self {
        Self::new_inner(
            name,
            doc_source,
            risk_level,
            requires_approval,
            true,
            doc_markdown,
            handler,
        )
    }

    fn new_inner(
        name: impl Into<String>,
        doc_source: &'static str,
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
            doc_source,
            doc_markdown: doc_markdown.into(),
            schema: None,
            handler,
            display: None,
        }
    }

    /// Register a tool with a standalone JSON Schema (from `.schema.yaml`), bypassing
    /// `doc_markdown` YAML frontmatter extraction.
    pub fn with_schema(mut self, schema: serde_json::Value) -> Self {
        self.schema = Some(schema);
        self
    }

    pub fn with_display(mut self, display: ToolDisplayFn) -> Self {
        self.display = Some(display);
        self
    }
}

#[derive(Debug, Clone)]
pub struct XmlToolDescriptor {
    pub name: String,
    pub doc_source: &'static str,
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

    /// Risk for a concrete invocation.
    pub fn tool_risk_level_for_invocation(&self, raw_tool_name: &str, _args: &Value) -> Option<String> {
        let base = registry_tool_base_name(raw_tool_name);
        self.tool_risk_level(base)
    }

    pub fn tool_requires_approval(&self, name: &str) -> bool {
        self.inner
            .read()
            .get(name)
            .is_some_and(|e| e.requires_approval)
    }

    /// For merged `file` tool, only `write` and `edit` need approval; other tools use registry flag.
    /// Now that file_write / file_edit are separate tools, the `file` special case is removed.
    pub fn tool_invocation_needs_approval(&self, tool_id: &str, _args: &Value) -> bool {
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
            .filter(|e| registry_tool_allowed(&e.def.name, allow))
            .map(|e| XmlToolDescriptor {
                name: e.def.name.clone(),
                doc_source: e.doc_source,
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
        let g = self.inner.read();
        let allowed: Vec<&ToolEntry> = g
            .values()
            .filter(|e| registry_tool_allowed(&e.def.name, allow))
            .collect();

        let mut source_counts: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for e in &allowed {
            *source_counts.entry(e.doc_source).or_default() += 1;
        }

        let mut out: Vec<serde_json::Value> = Vec::new();
        for e in allowed {
            let peers = *source_counts.get(e.doc_source).unwrap_or(&1);
            out.push(openai_tool_entry(
                &e.def.name,
                &openai_description_for_entry(&e.def.name, &e.doc_markdown, peers),
                openai_parameters_from_doc_or_builtin(
                    &e.def.name,
                    &e.doc_markdown,
                    e.schema.as_ref(),
                ),
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

/// Whether a registry tool id is allowed (exact id or family entry in `allow`).
fn registry_tool_allowed(registry_name: &str, allow: &[String]) -> bool {
    registry_tool_in_allow_list(allow, registry_name)
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

fn openai_compact_description(tool_name: &str) -> String {
    format!(
        "{tool_name}: parameters in schema; full usage in system Tools appendix."
    )
}

/// OpenAI `function.description`: compact when doc is shared or long; appendix holds full docs.
fn openai_description_for_entry(name: &str, doc: &str, peers_sharing_doc_source: usize) -> String {
    if peers_sharing_doc_source > 1 {
        return openai_compact_description(name);
    }
    let t = doc.trim();
    if t.is_empty() {
        return openai_compact_description(name);
    }
    const SHORT_DOC_MAX: usize = 240;
    if t.chars().count() <= SHORT_DOC_MAX {
        return t.to_string();
    }
    openai_compact_description(name)
}

fn openai_parameters_from_doc_or_builtin(name: &str, doc: &str, schema: Option<&serde_json::Value>) -> serde_json::Value {
    // Prefer standalone schema (from .schema.yaml) over YAML frontmatter extraction.
    if let Some(s) = schema {
        if s.is_object() {
            return s.clone();
        }
    }
    match json_schema_from_markdown(doc) {
        Ok(s) if s.is_object() => s,
        _ => panic!("Tool `{name}` missing valid schema in doc_markdown frontmatter"),
    }
}

#[cfg(test)]
mod parse_args_tests {
    use super::file_tool_effective_risk_level;
    use super::normalize_tool_invoke_name;
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
    fn registry_tool_base_name_is_trimmed_flat_id() {
        assert_eq!(registry_tool_base_name("mouse_click_index"), "mouse_click_index");
        assert_eq!(registry_tool_base_name("  file_read  "), "file_read");
    }

    #[test]
    fn normalize_tool_invoke_name_trims_only() {
        let args = serde_json::json!({"goal": "g"});
        let (id, out) = normalize_tool_invoke_name("  mouse_click_index  ", args.clone());
        assert_eq!(id, "mouse_click_index");
        assert_eq!(out, args);
    }

    #[test]
    fn file_risk_high_only_write_edit() {
        assert_eq!(
            file_tool_effective_risk_level("file_write", &serde_json::json!({})),
            "high"
        );
        assert_eq!(
            file_tool_effective_risk_level("file_edit", &serde_json::json!({})),
            "high"
        );
        assert_eq!(
            file_tool_effective_risk_level("file_read", &serde_json::json!({})),
            "low"
        );
        assert_eq!(
            file_tool_effective_risk_level("file_grep", &serde_json::json!({})),
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
    fn openai_tools_flat_file_tool_uses_standalone_schema() {
        let reg = ToolRegistry::new();
        let doc = "### `file_read`\nShared file doc.";
        // Without schema, test will panic (no YAML frontmatter). Provide one.
        reg.register(
            ToolEntry::new(
                "file_read",
                "test:file_read",
                "low",
                false,
                doc,
                Arc::new(|_| Ok(String::new())),
            )
            .with_schema(serde_json::json!({
                "type": "object",
                "properties": { "paths": { "type": "array" } },
                "required": ["paths"]
            })),
        );

        let tools = reg.openai_tools(&[]);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "file_read");
        let params = &tools[0]["function"]["parameters"];
        // Without schema, panics; with standalone schema field set, uses it.
        assert!(params["required"].as_array().is_some());
    }

    #[test]
    fn openai_tools_file_with_standalone_schema() {
        let reg = ToolRegistry::new();
        let doc = "### `file_write`\n-";
        reg.register(
            ToolEntry::new(
                "file_write",
                "test:file_write",
                "high",
                true,
                doc,
                Arc::new(|_| Ok(String::new())),
            )
                .with_schema(serde_json::json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" }, "content": {} },
                    "required": ["path", "content"]
                })),
        );

        let tools = reg.openai_tools(&[]);
        assert_eq!(tools.len(), 1);
        let params = &tools[0]["function"]["parameters"];
        assert_eq!(params["required"][0], "path");
    }

    #[test]
    fn openai_tools_skill_flat_uses_name() {
        let reg = ToolRegistry::new();
        reg.register(
            ToolEntry::new(
                "skill_load_instructions",
                "test:skill_load_instructions",
                "low",
                false,
                "### `skill_load_instructions`\n-",
                Arc::new(|_| Ok(String::new())),
            )
            .with_schema(serde_json::json!({
                "type": "object",
                "properties": { "skill_id": { "type": "string" } },
                "required": ["skill_id"]
            })),
        );
        let tools = reg.openai_tools(&[]);
        assert_eq!(tools[0]["function"]["name"], "skill_load_instructions");
    }

    #[test]
    fn openai_tools_task_board_flat_uses_name() {
        let reg = ToolRegistry::new();
        reg.register(
            ToolEntry::new(
                "task_board_patch",
                "test:task_board_patch",
                "low",
                false,
                "### `task_board_patch`\n-",
                Arc::new(|_| Ok(String::new())),
            )
            .with_schema(serde_json::json!({
                "type": "object",
                "properties": { "items": {} },
                "required": []
            })),
        );
        let tools = reg.openai_tools(&[]);
        assert_eq!(tools[0]["function"]["name"], "task_board_patch");
    }

    #[test]
    fn normalize_allowed_accepts_flat_name() {
        use super::normalize_allowed_tool_names;
        use std::collections::HashSet;

        let available: HashSet<String> = ["task_board_patch".into(), "file_read".into()].into_iter().collect();
        let mut names = vec!["task_board_patch".into(), "file_read".into()];
        normalize_allowed_tool_names(&mut names, &available);
        assert!(names.contains(&"task_board_patch".to_string()));
        assert!(names.contains(&"file_read".to_string()));
    }

    #[test]
    fn registry_tool_in_allow_list_accepts_qualified_entry() {
        use super::registry_tool_in_allow_list;
        let allow = vec!["task_board_patch".into()];
        assert!(registry_tool_in_allow_list(&allow, "task_board_patch"));
        assert!(!registry_tool_in_allow_list(&allow, "terminal"));
    }

    #[test]
    fn registry_tool_in_allow_list_accepts_captcha_family_for_flat_ids() {
        use super::registry_tool_in_allow_list;
        let allow = vec!["captcha_verify".into()];
        assert!(registry_tool_in_allow_list(&allow, "captcha_verify_click"));
        assert!(!registry_tool_in_allow_list(&allow, "mouse_click_index"));
    }

    #[test]
    fn registry_tool_in_allow_list_accepts_input_family_for_flat_ids() {
        use super::registry_tool_in_allow_list;
        let allow = vec!["input".into()];
        assert!(registry_tool_in_allow_list(&allow, "input_at"));
        assert!(registry_tool_in_allow_list(&allow, "input_focused"));
        assert!(!registry_tool_in_allow_list(&allow, "mouse_click_index"));
    }

    #[test]
    fn normalize_allowed_retains_desktop_tool_families() {
        use super::{normalize_allowed_tool_names, registry_tool_in_allow_list};
        use std::collections::HashSet;

        let available: HashSet<String> = [
            "mouse_click_index".into(),
            "input_at".into(),
            "input_focused".into(),
            "hotkey".into(),
        ]
        .into_iter()
        .collect();
        let mut names = vec![
            "mouse".into(),
            "input".into(),
            "mouse_click_index".into(),
            "input_at".into(),
            "hotkey".into(),
        ];
        normalize_allowed_tool_names(&mut names, &available);
        assert!(
            names.contains(&"input".to_string()),
            "family allow entry must survive normalize"
        );
        assert!(names.contains(&"mouse".to_string()));
        assert!(
            !names.contains(&"input_at".to_string()),
            "flat input ids collapse to family"
        );
        assert!(registry_tool_in_allow_list(&names, "input_at"));
        assert!(registry_tool_in_allow_list(&names, "input_focused"));
    }

    #[test]
    fn openai_tools_captcha_family_allow_exposes_flat_tools_with_schema() {
        use super::tool_doc::load_tools_from_schema_yaml;
        use super::ToolEntry;
        use super::ToolRegistry;
        use std::collections::HashSet;
        use std::sync::Arc;

        let yaml = include_str!("../agents/computer/tools/prompts/captcha_verify.schema.yaml");
        let schemas: std::collections::HashMap<String, serde_json::Value> =
            load_tools_from_schema_yaml(yaml).unwrap().into_iter().collect();
        let doc = include_str!("../agents/computer/tools/prompts/captcha_verify.md");
        let reg = ToolRegistry::new();
        for name in ["captcha_verify_type", "captcha_verify_click", "captcha_verify_drag"] {
            let schema = schemas.get(name).cloned().unwrap();
            reg.register(
                ToolEntry::new(
                    name,
                    "agents/computer/tools/prompts/captcha_verify.md",
                    "low",
                    false,
                    doc,
                    Arc::new(|_| Ok(String::new())),
                )
                .with_schema(schema),
            );
        }
        let allow = vec!["captcha_verify".into()];
        let tools = reg.openai_tools(&allow);
        assert_eq!(tools.len(), 3);
        let names: HashSet<_> = tools
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        assert!(names.contains("captcha_verify_click"));
        let click = tools
            .iter()
            .find(|t| t["function"]["name"] == "captcha_verify_click")
            .unwrap();
        assert!(click["function"]["parameters"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "goal"));
    }

    #[test]
    fn openai_tools_uses_compact_description_when_doc_source_shared() {
        use super::ToolEntry;
        use super::ToolRegistry;
        use std::sync::Arc;

        const MOUSE_DOC_SOURCE: &str = "agents/computer/tools/prompts/mouse.md";
        let reg = ToolRegistry::new();
        let shared = "### mouse family\nShared mouse doc body that would bloat API tools if repeated.";
        for name in ["mouse_click_index", "mouse_click_at", "mouse_hover_index"] {
            reg.register(
                ToolEntry::new(
                    name,
                    MOUSE_DOC_SOURCE,
                    "low",
                    false,
                    shared,
                    Arc::new(|_| Ok(String::new())),
                )
                .with_schema(serde_json::json!({
                    "type": "object",
                    "properties": { "goal": { "type": "string" } },
                    "required": ["goal"]
                })),
            );
        }

        let allow = vec!["mouse".into()];
        let tools = reg.openai_tools(&allow);
        assert_eq!(tools.len(), 3);
        for t in &tools {
            let desc = t["function"]["description"].as_str().unwrap();
            assert!(!desc.contains("Shared mouse doc"));
            assert!(desc.contains("system Tools appendix"));
            assert!(desc.starts_with("mouse_"));
        }
        let descs: std::collections::HashSet<_> = tools
            .iter()
            .filter_map(|t| t["function"]["description"].as_str())
            .collect();
        assert_eq!(descs.len(), 3, "each flat tool keeps a distinct compact description");
    }

    #[test]
    fn openai_tools_uses_builtin_schema_when_doc_has_no_json_fence() {
        let reg = ToolRegistry::new();
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "action": { "type": "string" },
                "index_captcha_area": { "type": "integer" },
                "is_slider": { "type": "boolean" },
                "is_click_block": { "type": "boolean" }
            },
            "required": ["action", "is_slider", "index_captcha_area", "is_click_block"]
        });
        reg.register(
            ToolEntry::new(
                "captcha_verify",
                "test:captcha_verify",
                "high",
                false,
                "plain doc without schema fence",
                Arc::new(|_| Ok(String::new())),
            )
            .with_schema(schema),
        );

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
        let batch = vec![tc("a", "task_board_patch"), tc("b", "terminal")];
        assert!(validate_envelope_tool_batch(&tools, &batch).is_ok());
    }

    #[test]
    fn batch_single_task_board_ok() {
        let tools = reg();
        let batch = vec![tc("a", "task_board_patch")];
        assert!(validate_envelope_tool_batch(&tools, &batch).is_ok());
    }

    #[test]
    fn batch_rejects_non_sidecar_prefix() {
        let tools = reg();
        let batch = vec![tc("a", "terminal"), tc("b", "file_read")];
        assert!(validate_envelope_tool_batch(&tools, &batch).is_err());
    }

    #[test]
    fn batch_all_sidecars_ok() {
        let tools = reg();
        let batch = vec![tc("a", "task_board_patch"), tc("b", "task_board_replace")];
        assert!(validate_envelope_tool_batch(&tools, &batch).is_ok());
    }

    #[test]
    fn batch_allows_sidecar_calls_around_single_primary() {
        let tools = reg();
        let batch = vec![
            tc("a", "task_board_patch"),
            tc("b", "terminal"),
            tc("c", "task_board_replace"),
        ];
        assert!(validate_envelope_tool_batch(&tools, &batch).is_ok());
    }

    #[test]
    fn batch_rejects_more_than_one_primary_tool() {
        let tools = reg();
        let batch = vec![tc("a", "terminal"), tc("b", "file_read"), tc("c", "task_board_patch")];
        assert!(validate_envelope_tool_batch(&tools, &batch).is_err());
    }
}
