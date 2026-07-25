//! Parse tool metadata from markdown with YAML frontmatter (for bundled tools that still embed metadata in `.md`).

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct ToolFrontmatter {
    id: String,
    #[serde(default = "default_risk_level")]
    risk_level: String,
    #[serde(default)]
    requires_approval: bool,
    parameters_schema_json: String,
}

fn default_risk_level() -> String {
    "medium".into()
}

/// Parsed tool markdown: runtime fields for [`super::ToolEntry`], excluding the handler.
#[derive(Debug, Clone)]
pub struct ParsedToolMarkdown {
    pub name: String,
    pub risk_level: String,
    pub requires_approval: bool,
    pub parameters_schema: Value,
    pub doc_markdown: String,
}

/// Parse `---` / YAML / `---` / body.
///
/// `expected_id` must match `id` in the frontmatter.
pub fn parse_tool_markdown(raw: &str, expected_id: &str) -> Result<ParsedToolMarkdown> {
    let (yaml_src, body) =
        split_yaml_frontmatter(raw).context("split tool markdown frontmatter")?;
    let fm: ToolFrontmatter =
        serde_yaml::from_str(yaml_src).context("parse tool YAML frontmatter")?;
    if fm.id != expected_id {
        anyhow::bail!("frontmatter id {:?} != expected {:?}", fm.id, expected_id);
    }
    let parameters_schema: Value = serde_json::from_str(fm.parameters_schema_json.trim())
        .context("parse parameters_schema_json as JSON")?;
    Ok(ParsedToolMarkdown {
        name: fm.id,
        risk_level: fm.risk_level,
        requires_approval: fm.requires_approval,
        parameters_schema,
        doc_markdown: body.trim().to_string(),
    })
}

fn split_yaml_frontmatter(raw: &str) -> Result<(&str, &str)> {
    let raw = raw.trim_start();
    if !raw.starts_with("---") {
        anyhow::bail!("expected file to start with --- YAML frontmatter");
    }
    let after_open = raw[3..].strip_prefix('\n').unwrap_or("");
    let term = "\n---\n";
    let end = after_open
        .find(term)
        .ok_or_else(|| anyhow::anyhow!("missing closing --- for YAML frontmatter"))?;
    let yaml_src = &after_open[..end];
    let body = after_open[end + term.len()..].trim_start();
    Ok((yaml_src, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let raw = r#"---
id: wait
risk_level: low
requires_approval: false
parameters_schema_json: '{"type":"object","properties":{"seconds":{"type":"integer"}},"required":["seconds"]}'
---
### wait
Hello."#;
        let p = parse_tool_markdown(raw, "wait").unwrap();
        assert_eq!(p.name, "wait");
        assert!(p.doc_markdown.contains("Hello"));
        assert_eq!(p.risk_level, "low");
        assert!(!p.requires_approval);
    }

    #[test]
    fn rejects_id_mismatch() {
        let raw = r#"---
id: mouse
risk_level: high
requires_approval: false
parameters_schema_json: '{"type":"object","properties":{},"required":[]}'
---
body"#;
        assert!(parse_tool_markdown(raw, "wait").is_err());
    }
}
