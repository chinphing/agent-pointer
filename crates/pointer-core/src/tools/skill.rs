use super::{ToolEntry, ToolHandler, ToolRegistry};
use crate::skills::SkillRegistry;
use anyhow::anyhow;
use std::sync::Arc;

/// Doc for registry tool `skill`; keep in sync with `prompts/skill.md`.
const SKILL_MD: &str = include_str!("prompts/skill.md");
/// Standalone schemas for flat skill tools (no `method` enum).
const SKILL_SCHEMA_YAML: &str = include_str!("prompts/skill.schema.yaml");

pub fn register_all(reg: &ToolRegistry, skills: Arc<SkillRegistry>) {
    let doc = super::tool_doc::doc_markdown_without_schema_fence(SKILL_MD);
    let schemas = super::tool_doc::load_tools_from_schema_yaml(SKILL_SCHEMA_YAML)
        .expect("skill.schema.yaml must be valid");
    let prompt = doc.trim().to_string();

    for (name, schema) in schemas {
        let sk = skills.clone();
        let handler: ToolHandler = match name.as_str() {
            "skill_load_instructions" => Arc::new(move |args| {
                let id = args
                    .get("skill_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("缺少 skill_id"))?;
                sk.load_instructions(id)
            }),
            "skill_read_resource" => Arc::new(move |args| {
                let id = args
                    .get("skill_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("缺少 skill_id"))?;
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("缺少 path"))?;
                sk.read_resource(id, path)
            }),
            _ => panic!("Unknown skill tool: {name}"),
        };

        reg.register(
            ToolEntry::new(name.clone(), "low", false, prompt.clone(), handler)
                .with_schema(schema),
        );
    }
}
