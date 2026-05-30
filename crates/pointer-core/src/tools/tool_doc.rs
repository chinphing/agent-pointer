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
    let yaml_v: serde_yaml::Value = serde_yaml::from_str(yaml_src)
        .map_err(|e| anyhow!("Invalid YAML front matter: {e}"))?;
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
    fn rejects_json_fence_schema_legacy_format() {
        let md = r#"# T

```json
{"type":"object"}
```
"#;
        let err = json_schema_from_markdown(md).unwrap_err().to_string();
        assert!(err.contains("YAML front matter"));
    }
}
