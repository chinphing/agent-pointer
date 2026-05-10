use super::ToolRegistry;
use crate::skills::SkillRegistry;
use crate::agents::computer::ComputerState;
use std::sync::Arc;

pub fn register_all(reg: &ToolRegistry) {
    crate::tools::terminal::register_all(reg);
    crate::tools::file::register_all(reg);
    crate::tools::response::register_all(reg);
}

pub fn register_skill_tools(reg: &ToolRegistry, skills: Arc<SkillRegistry>) {
    crate::tools::skill::register_all(reg, skills);
}

pub fn register_computer_tools(reg: &ToolRegistry, state: Arc<ComputerState>) {
    crate::agents::computer::tools::register_all(reg, state);
}
