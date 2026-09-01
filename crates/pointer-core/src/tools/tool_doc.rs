//! Load tool JSON Schema from markdown YAML front matter (`---`) with top-level `schema`.

use anyhow::{anyhow, Result};
use serde_json::Value;

fn split_yaml_front_matter(md: &str) -> Option<(&str, &str)> {
    let s = md.trim_start_matches('\u{feff}');
    if !s.starts_with("---\n") {
        return None;
    }
    let rest = &s[4..];
    let end = rest.find("\n---\n")?;
    let yaml = &rest[..end];
    let body = &rest[end + 5..];
    Some((yaml, body))
}

fn schema_from_yaml_front_matter(md: &str) -> Result<Option<Value>> {
    let Some((yaml_src, _body)) = split_yaml_front_matter(md) else {
        return Ok(None);
    };
    let yaml_v: serde_yaml::Value =
        serde_yaml::from_str(yaml_src).map_err(|e| anyhow!("Invalid YAML front matter: {e}"))?;
    let Some(schema_yaml) = yaml_v.get("schema") else {
        return Ok(None);
    };
    let schema = serde_json::to_value(schema_yaml)
        .map_err(|e| anyhow!("Invalid schema in YAML front matter: {e}"))?;
    Ok(Some(schema))
}

/// Parse schema from YAML front matter (`schema`) only.
pub fn json_schema_from_markdown(md: &str) -> Result<Value> {
    schema_from_yaml_front_matter(md)?
        .ok_or_else(|| anyhow!("Markdown has no YAML front matter `schema`"))
}

/// Markdown documentation with YAML front matter schema removed (for `doc_markdown`).
pub fn doc_markdown_without_schema_fence(md: &str) -> String {
    if let Some((yaml_src, body)) = split_yaml_front_matter(md) {
        if let Ok(yaml_v) = serde_yaml::from_str::<serde_yaml::Value>(yaml_src) {
            if yaml_v.get("schema").is_some() {
                return body.trim().to_string();
            }
        }
    }
    md.trim().to_string()
}

/// Combined load for callers that still embed JSON Schema in markdown (not used by current registry `ToolEntry` wiring).
pub fn load_tool_doc_and_schema(md: &str) -> Result<(Value, String)> {
    let schema = json_schema_from_markdown(md)?;
    let doc = doc_markdown_without_schema_fence(md);
    Ok((schema, doc))
}

/// Parse a `.schema.yaml` file whose top-level keys are flat tool names and each value is
/// the tool's JSON Schema (no `method` enum). Returns `Vec<(tool_name, json_schema)>`.
///
/// Example YAML:
/// ```yaml
/// file_read:
///   type: object
///   properties:
///     paths:
///       type: array
///   required: [paths]
/// file_write:
///   type: object
///   properties:
///     path:
///       type: string
///     content: {}
///   required: [path, content]
/// ```
pub fn load_tools_from_schema_yaml(yaml_str: &str) -> Result<Vec<(String, Value)>> {
    let yaml_v: serde_yaml::Value =
        serde_yaml::from_str(yaml_str).map_err(|e| anyhow!("Invalid schema YAML: {e}"))?;
    let mapping = yaml_v
        .as_mapping()
        .ok_or_else(|| anyhow!("Schema YAML top-level must be a mapping"))?;
    let mut tools = Vec::with_capacity(mapping.len());
    for (key, value) in mapping {
        let name = key
            .as_str()
            .ok_or_else(|| anyhow!("Schema YAML keys must be strings"))?
            .to_string();
        let schema = serde_json::to_value(value)
            .map_err(|e| anyhow!("Invalid JSON schema for tool '{name}': {e}"))?;
        tools.push((name, schema));
    }
    Ok(tools)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_schema_from_yaml_front_matter() {
        let md = r#"---
schema:
  type: object
  properties:
    x:
      type: integer
  required:
    - x
---

# T

Body.
"#;
        let v = json_schema_from_markdown(md).unwrap();
        assert_eq!(v["type"], "object");
        assert_eq!(v["required"][0], "x");
        let d = doc_markdown_without_schema_fence(md);
        assert!(d.starts_with("# T"));
        assert!(!d.contains("schema:"));
    }

    #[test]
    fn terminal_schema_cwd_defaults_to_workspace() {
        let md = include_str!("prompts/terminal.md");
        let v = json_schema_from_markdown(md).unwrap();
        let cwd = v["properties"]["cwd"]["description"].as_str().unwrap();
        assert!(cwd.contains("$WORKING_DIR"));
        let command = v["properties"]["command"]["description"].as_str().unwrap();
        assert!(command.contains("$WORKING_DIR"));
        assert!(command.contains("All reads, writes"));
    }

    #[test]
    fn rejects_json_fence_schema_legacy_format() {
        let md = r#"# T

```json
{"type":"object"}
```
"#;
        let err = json_schema_from_markdown(md).unwrap_err().to_string();
        assert!(err.contains("YAML front matter"));
    }

    #[test]
    fn loads_flat_tools_from_schema_yaml() {
        let yaml = r#"
file_read:
  type: object
  properties:
    paths:
      type: array
  required: [paths]
file_write:
  type: object
  properties:
    path:
      type: string
    content: {}
  required: [path, content]
"#;
        let tools = load_tools_from_schema_yaml(yaml).unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].0, "file_read");
        assert_eq!(tools[1].0, "file_write");
        assert_eq!(tools[0].1["type"], "object");
        assert_eq!(tools[0].1["required"][0], "paths");
        assert_eq!(tools[1].1["required"][0], "path");
    }

    #[test]
    fn schema_yaml_rejects_non_mapping_top_level() {
        let yaml = "- item1\n- item2\n";
        let err = load_tools_from_schema_yaml(yaml).unwrap_err().to_string();
        assert!(err.contains("mapping"));
    }

    /// Verify our actual .schema.yaml files parse correctly.
    #[test]
    fn real_file_schema_yaml_parses() {
        let yaml_str = include_str!("prompts/file.schema.yaml");
        let tools = load_tools_from_schema_yaml(yaml_str).unwrap();
        let names: Vec<&str> = tools.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "file_read",
                "file_write",
                "file_edit",
                "file_glob",
                "file_grep",
                "file_list"
            ]
        );
        for (name, schema) in &tools {
            assert_eq!(schema["type"], "object", "tool {name} missing type: object");
        }
        let file_read = tools
            .iter()
            .find(|(n, _)| n == "file_read")
            .map(|(_, s)| s)
            .expect("file_read schema");
        assert_eq!(file_read["properties"]["offset"]["type"], "integer");
        assert_eq!(file_read["properties"]["offset"]["default"], 1);
        assert_eq!(file_read["properties"]["limit"]["type"], "integer");
        assert_eq!(file_read["properties"]["limit"]["default"], 500);
        assert_eq!(file_read["properties"]["limit"]["maximum"], 2000);
        assert!(
            file_read["properties"].get("lineStart").is_none(),
            "do not advertise lineStart; offset/limit is the canonical window"
        );
        assert!(
            file_read["properties"].get("startLine").is_none(),
            "do not advertise startLine; offset/limit is the canonical window"
        );
        assert_eq!(file_read["additionalProperties"], false);
        let file_grep = tools
            .iter()
            .find(|(n, _)| n == "file_grep")
            .map(|(_, s)| s)
            .expect("file_grep schema");
        assert_eq!(file_grep["properties"]["limit"]["type"], "integer");
        assert_eq!(file_grep["properties"]["limit"]["default"], 50);
        assert_eq!(file_grep["properties"]["limit"]["maximum"], 200);
        assert!(
            file_grep["properties"].get("maxResults").is_none(),
            "file_grep hit cap is limit, not maxResults"
        );
        let file_glob = tools
            .iter()
            .find(|(n, _)| n == "file_glob")
            .map(|(_, s)| s)
            .expect("file_glob schema");
        assert_eq!(file_glob["properties"]["limit"]["type"], "integer");
        assert_eq!(file_glob["properties"]["limit"]["default"], 100);
        assert_eq!(file_glob["properties"]["limit"]["maximum"], 500);
        assert!(
            file_glob["properties"].get("maxResults").is_none(),
            "file_glob hit cap is limit, not maxResults"
        );
        let file_list = tools
            .iter()
            .find(|(n, _)| n == "file_list")
            .map(|(_, s)| s)
            .expect("file_list schema");
        assert_eq!(file_list["properties"]["limit"]["type"], "integer");
        assert_eq!(file_list["properties"]["limit"]["default"], 100);
        assert_eq!(file_list["properties"]["limit"]["maximum"], 2000);
        assert!(
            file_list["properties"].get("maxResults").is_none(),
            "file_list hit cap is limit, not maxResults"
        );
    }

    #[test]
    fn real_skill_schema_yaml_parses() {
        let yaml_str = include_str!("prompts/skill.schema.yaml");
        let tools = load_tools_from_schema_yaml(yaml_str).unwrap();
        let names: Vec<&str> = tools.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["skill_read", "skill_import"]);
    }

    #[test]
    fn real_input_schema_yaml_parses() {
        let yaml_str = include_str!("../agents/computer/tools/prompts/input.schema.yaml");
        let tools = load_tools_from_schema_yaml(yaml_str).unwrap();
        let names: Vec<&str> = tools.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["input_index", "input_at", "input_focused"]);
    }

    #[test]
    fn real_mouse_schema_yaml_parses() {
        let yaml_str = include_str!("../agents/computer/tools/prompts/mouse.schema.yaml");
        let tools = load_tools_from_schema_yaml(yaml_str).unwrap();
        let names: Vec<&str> = tools.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"mouse_double_click_index"));
        assert!(names.contains(&"mouse_scroll_current"));
        assert!(names.contains(&"mouse_scroll_index"));
    }
}
