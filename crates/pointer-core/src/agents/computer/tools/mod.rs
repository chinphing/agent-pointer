//! Computer-use **tool handlers** registered with [`crate::tools::ToolRegistry`].
//!
//! Sibling modules (`actions`, `screen`, [`crate::agents::computer::ComputerState`], etc.) live in the parent [`crate::agents::computer`] package.

pub mod args_util;
mod tool_composite;
mod tool_computer;
mod tool_hotkey;
mod tool_modified_click;
mod tool_mouse;
mod tool_wait;

use crate::agents::computer::ComputerState;
use crate::tools::{ToolEntry, ToolRegistry};
use std::sync::Arc;
use tool_modified_click::ModifiedClickTool;

/// Register all computer-use tools (mouse, hotkey, composite_action, modified_click, computer, wait).
pub fn register_all(reg: &ToolRegistry, state: Arc<ComputerState>) {
    let mouse_state = state.clone();
    reg.register(ToolEntry::new(
        "mouse",
        "high",
        false,
        serde_json::from_str(include_str!("schemas/mouse.json")).expect("schemas/mouse.json"),
        include_str!("prompts/mouse.md").trim(),
        None,
        Arc::new(move |args| {
            let method = args["method"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'method' parameter"))?
                .to_string();
            let tool = tool_mouse::MouseTool::new(
                mouse_state.executor.clone(),
                mouse_state.vision_state.clone(),
            );
            tool.execute(&method, &args)
        }),
    ));

    let hotkey_state = state.clone();
    reg.register(ToolEntry::new(
        "hotkey",
        "medium",
        false,
        serde_json::from_str(include_str!("schemas/hotkey.json")).expect("schemas/hotkey.json"),
        include_str!("prompts/hotkey.md").trim(),
        None,
        Arc::new(move |args| {
            let tool = tool_hotkey::HotkeyTool::new(hotkey_state.executor.clone());
            tool.execute("hotkey", &args)
        }),
    ));

    let composite_state = state.clone();
    reg.register(ToolEntry::new(
        "composite_action",
        "high",
        false,
        serde_json::from_str(include_str!("schemas/composite_action.json"))
            .expect("schemas/composite_action.json"),
        include_str!("prompts/composite_action.md").trim(),
        None,
        Arc::new(move |args| {
            let method = args["method"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'method' parameter"))?
                .to_string();
            let tool = tool_composite::CompositeActionTool::new(
                composite_state.executor.clone(),
                composite_state.vision_state.clone(),
            );
            tool.execute(&method, &args)
        }),
    ));

    let modified_state = state.clone();
    reg.register(ToolEntry::new(
        "modified_click",
        "high",
        false,
        serde_json::from_str(include_str!("schemas/modified_click.json"))
            .expect("schemas/modified_click.json"),
        include_str!("prompts/modified_click.md").trim(),
        None,
        Arc::new(move |args| {
            let method = args["method"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'method' parameter"))?
                .to_string();
            let tool = ModifiedClickTool::new(
                modified_state.executor.clone(),
                modified_state.vision_state.clone(),
            );
            tool.execute(&method, &args)
        }),
    ));

    reg.register(ToolEntry::new(
        "computer",
        "low",
        false,
        serde_json::from_str(include_str!("schemas/computer.json")).expect("schemas/computer.json"),
        include_str!("prompts/computer.md").trim(),
        None,
        Arc::new(move |args| {
            let method = args["method"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'method' parameter"))?
                .to_string();
            let tool = tool_computer::ComputerMetaTool;
            tool.execute(&method, &args)
        }),
    ));

    reg.register(ToolEntry::new(
        "wait",
        "low",
        false,
        serde_json::from_str(include_str!("schemas/wait.json")).expect("schemas/wait.json"),
        include_str!("prompts/wait.md").trim(),
        None,
        Arc::new(move |args| {
            let tool = tool_wait::WaitTool::new();
            tool.execute("wait", &args)
        }),
    ));
}
