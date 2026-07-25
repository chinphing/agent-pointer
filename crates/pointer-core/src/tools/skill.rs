use super::{ToolEntry, ToolHandler, ToolRegistry};
use crate::skills::SkillRegistry;
use anyhow::anyhow;
use std::sync::Arc;

/// Doc for registry tool `skill`; keep in sync with `prompts/skill.md`.
const SKILL_MD: &str = include_str!("prompts/skill.md");
const SKILL_DOC_SOURCE: &str = "tools/prompts/skill.md";
/// Standalone schemas for flat skill tools (no `method` enum).
const SKILL_SCHEMA_YAML: &str = include_str!("prompts/skill.schema.yaml");

pub fn register_all(reg: &ToolRegistry, skills: Arc<SkillRegistry>) {
    let doc = super::tool_doc::doc_markdown_without_schema_fence(SKILL_MD);
    let schemas = super::tool_doc::load_tools_from_schema_yaml(SKILL_SCHEMA_YAML)
        .expect("skill.schema.yaml must be valid");
    let prompt = doc.trim().to_string();

    for (name, schema) in schemas {
        let sk = skills.clone();
        let (risk, requires_approval) = if name == "skill_import" {
            ("medium", true)
        } else {
            ("low", false)
        };
        let handler: ToolHandler = match name.as_str() {
            "skill_read" => Arc::new(move |args| {
                let id = args
                    .get("skill_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("缺少 skill_id"))?;
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .ok_or_else(|| {
                        anyhow!("缺少 path（读说明传 SKILL.md；读资源传 skill 相对路径）")
                    })?;
                sk.read(id, path)
            }),
            "skill_import" => Arc::new(move |args| {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("缺少 path"))?;
                let resolved = SkillRegistry::resolve_import_source(path)?;
                let result = sk.import_path(&resolved)?;
                if result.imported.is_empty() {
                    let detail = if result.skipped.is_empty() {
                        "未导入任何 Skill".to_string()
                    } else {
                        format!(
                            "未导入任何 Skill；跳过 {} 项：{}",
                            result.skipped.len(),
                            result.skipped.join("; ")
                        )
                    };
                    return Err(anyhow!(detail));
                }
                Ok(serde_json::to_string(&result)?)
            }),
            _ => panic!("Unknown skill tool: {name}"),
        };

        let entry = ToolEntry::new(
            name.clone(),
            SKILL_DOC_SOURCE,
            risk,
            requires_approval,
            prompt.clone(),
            handler,
        )
        .with_schema(schema);
        let entry = if name == "skill_import" {
            entry.with_subagent_inheritance(false)
        } else {
            entry
        };
        reg.register(entry);
    }
}
