use super::{ToolEntry, ToolHandler, ToolRegistry};
use crate::skills::SkillRegistry;
use anyhow::{anyhow, Result};
use std::sync::Arc;

/// Doc for registry tool `skill`; keep in sync with `prompts/skill.md`.
const SKILL_MD: &str = include_str!("prompts/skill.md");

fn args_without_method(args: &serde_json::Value) -> serde_json::Value {
    match args {
        serde_json::Value::Object(m) => {
            let mut m = m.clone();
            m.remove("method");
            serde_json::Value::Object(m)
        }
        _ => args.clone(),
    }
}

pub fn register_all(reg: &ToolRegistry, skills: Arc<SkillRegistry>) {
    let doc = SKILL_MD.trim();
    let skills_clone = skills.clone();
    let h: ToolHandler = Arc::new(move |args| execute_skill_tool(&args, &skills_clone));
    reg.register(ToolEntry::new(
        "skill",
        "low",
        false,
        doc,
        h,
    ));
}

fn execute_skill_tool(args: &serde_json::Value, skills: &SkillRegistry) -> Result<String> {
    let method = args
        .get("method")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            anyhow!(
                "缺少 method；或使用限定名 skill:load_instructions / skill:read_resource"
            )
        })?;
    let payload = args_without_method(args);
    match method {
        "load_instructions" => {
            let id = payload
                .get("skill_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("缺少 skill_id"))?;
            skills.load_instructions(id)
        }
        "read_resource" => {
            let id = payload
                .get("skill_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("缺少 skill_id"))?;
            let path = payload
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("缺少 path"))?;
            skills.read_resource(id, path)
        }
        _ => Err(anyhow!(
            "未知 skill.method: {method}（允许 load_instructions | read_resource）"
        )),
    }
}
