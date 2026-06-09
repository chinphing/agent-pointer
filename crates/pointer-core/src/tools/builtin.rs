use super::ToolRegistry;
use crate::skills::SkillRegistry;
use crate::agents::computer::ComputerState;
use std::sync::Arc;

pub fn register_all(reg: &ToolRegistry, task_board_store: Arc<crate::task_board::TaskBoardStore>) {
    crate::tools::terminal::register_all(reg);
    crate::agents::coder::read_lints::register_all(reg);
    crate::tools::file::register_all(reg);
    crate::tools::web_search::register_all(reg);
    crate::tools::run_subagent::register_all(reg);
    crate::task_board::register_task_board_tool(reg, task_board_store);
}

pub fn register_skill_tools(reg: &ToolRegistry, skills: Arc<SkillRegistry>) {
    crate::tools::skill::register_all(reg, skills);
}

pub fn register_computer_tools(reg: &ToolRegistry, state: Arc<ComputerState>) {
    crate::agents::computer::tools::register_all(reg, state);
}
