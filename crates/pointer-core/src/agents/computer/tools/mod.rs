//! Computer-use **tool handlers** registered with [`crate::tools::ToolRegistry`].
//!
//! Sibling modules (`actions`, `screen`, [`crate::agents::computer::ComputerState`], etc.) live in the parent [`crate::agents::computer`] package.

pub mod args_util;
pub mod tool_prompts;
mod tool_clipboard;
mod tool_composite;
mod tool_hotkey;
mod tool_modified_click;
mod tool_mouse;
mod tool_wait;

use crate::agents::computer::ComputerState;
use args_util::effective_human_like_default;
use crate::platform::run_synthetic_input;
use crate::tools::{ToolEntry, ToolRegistry};
use std::sync::Arc;
use tool_modified_click::ModifiedClickTool;

/// Extract the authoritative `_conversation_id` from tool arguments injected by `chat_service`.
fn conversation_id_from_args(args: &serde_json::Value) -> Option<&str> {
    args.as_object()
        .and_then(|m| m.get("_conversation_id"))
        .and_then(|v| v.as_str())
}

/// Register all computer-use tools (mouse, hotkey, composite_action, modified_click, wait, clipboard).
pub fn register_all(reg: &ToolRegistry, state: Arc<ComputerState>) {
    let mouse_state = state.clone();
    let mouse_doc = include_str!("prompts/coordinate/mouse.md").trim();
    reg.register(ToolEntry::new(
        "mouse",
        "high",
        false,
        mouse_doc,
        Arc::new(move |args| {
            let mouse_state = mouse_state.clone();
            let cid = conversation_id_from_args(&args)
                .unwrap_or_default()
                .to_string();
            run_synthetic_input(move || {
                let method = args["method"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'method' parameter"))?
                    .to_string();
                let vision = mouse_state.vision_state_for_conversation(&cid);
                let hl_default =
                    effective_human_like_default(mouse_state.human_like_default);
                let tool = tool_mouse::MouseTool::new(
                    mouse_state.executor.clone(),
                    vision,
                    hl_default,
                );
                tool.execute(&method, &args)
            })
        }),
    ));

    let hotkey_state = state.clone();
    let hotkey_doc = include_str!("prompts/hotkey.md").trim();
    reg.register(ToolEntry::new(
        "hotkey",
        "medium",
        false,
        hotkey_doc,
        Arc::new(move |args| {
            let hotkey_state = hotkey_state.clone();
            run_synthetic_input(move || {
                let tool = tool_hotkey::HotkeyTool::new(hotkey_state.executor.clone());
                tool.execute("hotkey", &args)
            })
        }),
    ));

    let composite_state = state.clone();
    let composite_doc = include_str!("prompts/coordinate/composite_action.md").trim();
    reg.register(ToolEntry::new(
        "composite_action",
        "high",
        false,
        composite_doc,
        Arc::new(move |args| {
            let composite_state = composite_state.clone();
            let cid = conversation_id_from_args(&args)
                .unwrap_or_default()
                .to_string();
            run_synthetic_input(move || {
                let method = args["method"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'method' parameter"))?
                    .to_string();
                let vision = composite_state.vision_state_for_conversation(&cid);
                let hl_default =
                    effective_human_like_default(composite_state.human_like_default);
                let tool = tool_composite::CompositeActionTool::new(
                    composite_state.executor.clone(),
                    vision,
                    hl_default,
                );
                tool.execute(&method, &args)
            })
        }),
    ));

    let modified_state = state.clone();
    let modified_doc = include_str!("prompts/coordinate/modified_click.md").trim();
    reg.register(ToolEntry::new(
        "modified_click",
        "high",
        false,
        modified_doc,
        Arc::new(move |args| {
            let modified_state = modified_state.clone();
            let cid = conversation_id_from_args(&args)
                .unwrap_or_default()
                .to_string();
            run_synthetic_input(move || {
                let method = args["method"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'method' parameter"))?
                    .to_string();
                let vision = modified_state.vision_state_for_conversation(&cid);
                let hl_default =
                    effective_human_like_default(modified_state.human_like_default);
                let tool = ModifiedClickTool::new(
                    modified_state.executor.clone(),
                    vision,
                    hl_default,
                );
                tool.execute(&method, &args)
            })
        }),
    ));

    let wait_doc = include_str!("prompts/wait.md").trim();
    reg.register(ToolEntry::new(
        "wait",
        "low",
        false,
        wait_doc,
        Arc::new(move |args| {
            let tool = tool_wait::WaitTool::new();
            tool.execute("wait", &args)
        }),
    ));

    let clipboard_doc = include_str!("prompts/clipboard.md").trim();
    reg.register(ToolEntry::new(
        "clipboard",
        "medium",
        false,
        clipboard_doc,
        Arc::new(move |args| {
            let method = args["method"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'method' parameter"))?
                .to_string();
            let tool = tool_clipboard::ClipboardTool::new();
            tool.execute(&method, &args)
        }),
    ));
}
