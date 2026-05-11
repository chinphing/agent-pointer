//! Load JSON Schema embedded in Markdown (` ```json ... ``` `).

use anyhow::{anyhow, Result};
use serde_json::Value;

/// Parse the first fenced ` ```json ` … ` ``` ` block as a JSON Schema [`Value`].
pub fn json_schema_from_markdown(md: &str) -> Result<Value> {
    let key = "```json";
    let start = md.find(key).ok_or_else(|| anyhow!("Markdown has no ```json schema fence"))?;
    let after = &md[start + key.len()..];
    let after = after.strip_prefix('\r').unwrap_or(after);
    let after = after.strip_prefix('\n').unwrap_or(after);
    let close = after
        .find("\n```")
        .or_else(|| after.find("```"))
        .ok_or_else(|| anyhow!("Unclosed ```json schema fence"))?;
    let json_src = after[..close].trim();
    serde_json::from_str(json_src).map_err(|e| anyhow!("Invalid JSON in schema fence: {e}"))
}

/// Markdown documentation with the first ` ```json ` schema fence removed (for `doc_markdown`).
pub fn doc_markdown_without_schema_fence(md: &str) -> String {
    let key = "```json";
    let Some(start) = md.find(key) else {
        return md.trim().to_string();
    };
    let rest = &md[start + key.len()..];
    let rest = rest.strip_prefix('\r').unwrap_or(rest);
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let end = match rest.find("```") {
        Some(i) => i,
        None => return md.trim().to_string(),
    };
    let tail = rest[end + 3..].trim_start();
    let head = md[..start].trim_end();
    match (head.is_empty(), tail.is_empty()) {
        (true, true) => String::new(),
        (true, false) => tail.to_string(),
        (false, true) => head.to_string(),
        (false, false) => format!("{head}\n\n{tail}").trim().to_string(),
    }
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
    fn extracts_schema_and_strips_fence() {
        let md = r#"# T

Body.

```json
{"type":"object","properties":{"x":{"type":"integer"}}}
```

Tail."#;
        let v = json_schema_from_markdown(md).unwrap();
        assert_eq!(v["type"], "object");
        let d = doc_markdown_without_schema_fence(md);
        assert!(d.contains("Body"));
        assert!(d.contains("Tail"));
        assert!(!d.contains("```json"));
    }
}
