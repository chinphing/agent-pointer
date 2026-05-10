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
use crate::tools::{load_tool_doc_and_schema, ToolEntry, ToolRegistry};
use std::sync::Arc;
use tool_modified_click::ModifiedClickTool;

/// Register all computer-use tools (mouse, hotkey, composite_action, modified_click, computer, wait).
pub fn register_all(reg: &ToolRegistry, state: Arc<ComputerState>) {
    let mouse_state = state.clone();
    let (mouse_schema, mouse_doc) =
        load_tool_doc_and_schema(include_str!("prompts/mouse.md")).expect("prompts/mouse.md schema");
    reg.register(ToolEntry::new(
        "mouse",
        "high",
        false,
        mouse_schema,
        mouse_doc.trim(),
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
    let (hotkey_schema, hotkey_doc) =
        load_tool_doc_and_schema(include_str!("prompts/hotkey.md")).expect("prompts/hotkey.md schema");
    reg.register(ToolEntry::new(
        "hotkey",
        "medium",
        false,
        hotkey_schema,
        hotkey_doc.trim(),
        None,
        Arc::new(move |args| {
            let tool = tool_hotkey::HotkeyTool::new(hotkey_state.executor.clone());
            tool.execute("hotkey", &args)
        }),
    ));

    let composite_state = state.clone();
    let (composite_schema, composite_doc) =
        load_tool_doc_and_schema(include_str!("prompts/composite_action.md"))
            .expect("prompts/composite_action.md schema");
    reg.register(ToolEntry::new(
        "composite_action",
        "high",
        false,
        composite_schema,
        composite_doc.trim(),
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
    let (modified_schema, modified_doc) =
        load_tool_doc_and_schema(include_str!("prompts/modified_click.md"))
            .expect("prompts/modified_click.md schema");
    reg.register(ToolEntry::new(
        "modified_click",
        "high",
        false,
        modified_schema,
        modified_doc.trim(),
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

    let (computer_schema, computer_doc) =
        load_tool_doc_and_schema(include_str!("prompts/computer.md")).expect("prompts/computer.md schema");
    reg.register(ToolEntry::new(
        "computer",
        "low",
        false,
        computer_schema,
        computer_doc.trim(),
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

    let (wait_schema, wait_doc) =
        load_tool_doc_and_schema(include_str!("prompts/wait.md")).expect("prompts/wait.md schema");
    reg.register(ToolEntry::new(
        "wait",
        "low",
        false,
        wait_schema,
        wait_doc.trim(),
        None,
        Arc::new(move |args| {
            let tool = tool_wait::WaitTool::new();
            tool.execute("wait", &args)
        }),
    ));
}
