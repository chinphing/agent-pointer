use super::ToolRegistry;
use crate::skills::SkillRegistry;
use std::sync::Arc;

pub fn register_all(reg: &ToolRegistry) {
    crate::tools::math::register_all(reg);
    crate::tools::text::register_all(reg);
    crate::tools::general::register_all(reg);
    crate::tools::terminal::register_all(reg);
    crate::tools::workspace::register_all(reg);
}

pub fn register_skill_tools(reg: &ToolRegistry, skills: Arc<SkillRegistry>) {
    crate::tools::skills::register_all(reg, skills);
}
