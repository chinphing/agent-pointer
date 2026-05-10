use super::{ToolEntry, ToolHandler, ToolPrompt, ToolRegistry};
use crate::skills::SkillRegistry;
use anyhow::anyhow;
use std::sync::Arc;

const SKILLS_PROMPT: &str = include_str!("prompts/skills.md");

pub fn register_all(reg: &ToolRegistry, skills: Arc<SkillRegistry>) {
    register_load_skill_instructions(reg, skills.clone());
    register_read_skill_resource(reg, skills);
}

fn register_load_skill_instructions(reg: &ToolRegistry, skills: Arc<SkillRegistry>) {
    let h: ToolHandler = Arc::new(move |args| {
        let id = args
            .get("skill_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("缺少 skill_id"))?;
        skills.load_instructions(id)
    });
    reg.register(ToolEntry::new(
        "load_skill_instructions",
        "low",
        false,
        serde_json::json!({
            "type":"object",
            "properties":{ "skill_id":{"type":"string","description":"要加载的 Skill id"} },
            "required":["skill_id"]
        }),
        "加载指定 Skill 的 SKILL.md 正文说明。",
        Some(ToolPrompt {
            system_prompt: SKILLS_PROMPT.into(),
        }),
        h,
    ));
}

fn register_read_skill_resource(reg: &ToolRegistry, skills: Arc<SkillRegistry>) {
    let h: ToolHandler = Arc::new(move |args| {
        let id = args
            .get("skill_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("缺少 skill_id"))?;
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("缺少 path"))?;
        skills.read_resource(id, path)
    });
    reg.register(ToolEntry::new(
        "read_skill_resource",
        "low",
        false,
        serde_json::json!({
            "type":"object",
            "properties":{
                "skill_id":{"type":"string","description":"Skill id"},
                "path":{"type":"string","description":"资源相对路径，例如 references/api-guide.md"}
            },
            "required":["skill_id", "path"]
        }),
        "读取指定 Skill 的资源文件内容。",
        Some(ToolPrompt {
            system_prompt: SKILLS_PROMPT.into(),
        }),
        h,
    ));
}
