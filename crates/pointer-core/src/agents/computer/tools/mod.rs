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
mod tool_tier_signal;
mod tool_wait;

use crate::agents::computer::ComputerState;
use crate::agents::computer::tier::{tier_allows_index_tools, ComputerTier, ComputerTierGuard};
use args_util::{effective_human_like_default, ensure_index_method_allowed, method_uses_overlay_index};
use crate::platform::run_synthetic_input;
use crate::tools::{ToolEntry, ToolRegistry};
use anyhow::Result;
use std::sync::Arc;
use tool_modified_click::ModifiedClickTool;

const PRIMARY_MOUSE_DOC: &str = r#"### mouse

Use for a single mouse action: click, double-click, right-click, hover, drag, scroll at current cursor, or a small offset move.

Primary may use both index and coordinate methods. Pick one route per call from tier communication.

Index methods: `mouse:click_index`, `mouse:double_click_index`, `mouse:right_click_index`, `mouse:hover_index`, `mouse:drag_from_to_index`.
Coordinate methods: `mouse:click_at`, `mouse:double_click_at`, `mouse:right_click_at`, `mouse:hover_at`, `mouse:drag_from_to_at`.
Current cursor methods: `mouse:click_current`, `mouse:double_click_current`, `mouse:right_click_current`, `mouse:scroll_at_current`, `mouse:move_offset`.

Parameter constraints:
- `goal` and `action` are required.
- Use either index args (`index`/`from_index`/`to_index`) or coordinate args (`x/y` or `x1/y1/x2/y2`) in one call.
- Do not mix index and coordinate args in one call.

Optional `wait` in `tool_args`: 1-5 seconds.
"#;

const PRIMARY_COMPOSITE_DOC: &str = r#"### composite_action

Use for typing and indexed scroll actions.

Primary may use both index and coordinate methods. Pick one route per call from tier communication.

Methods:
- `composite_action:type_text_at_index`
- `composite_action:type_text_at`
- `composite_action:type_text_at_focused`
- `composite_action:scroll_at_index`

Parameter constraints:
- `goal` and `action` are required.
- `text` is required for type methods.
- `auto_enter` defaults to false.
- Use either index args or coordinate args in one call; do not mix.

Optional `wait` in `tool_args`: 1-5 seconds.
"#;

const PRIMARY_MODIFIED_CLICK_DOC: &str = r#"### modified_click

Use for multi-select or range-select click operations.

Primary may use both index and coordinate methods. Pick one route per call from tier communication.

Methods:
- `modified_click:modified_click_index`
- `modified_click:modified_click_at`

Parameter constraints:
- `goal` and `action` are required.
- `range_select=true` requires exactly two targets.
- Index route uses `indices`; coordinate route uses `positions`.

Optional `wait` in `tool_args`: 1-5 seconds.
"#;

/// Reject index methods when the conversation tier is coordinate-only (Advanced).
fn ensure_method_allowed_for_tier(tier: ComputerTier, method: &str) -> Result<()> {
    if method_uses_overlay_index(method) && !tier_allows_index_tools(tier) {
        anyhow::bail!(
            "Index-based method `{method}` is disabled at tier `{}`; use coordinate methods (*_at) with session x/y from Overlay reference bboxes.",
            tier.label()
        );
    }
    Ok(())
}

/// Run synthetic input on the platform main thread when required, with tier context set there.
fn run_synthetic_computer_tool<R, F>(tier: ComputerTier, f: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    run_synthetic_input(move || {
        let _guard = ComputerTierGuard::enter(tier);
        f()
    })
}

/// Extract the authoritative `_conversation_id` from tool arguments injected by `chat_service`.
fn conversation_id_from_args(args: &serde_json::Value) -> Option<&str> {
    args.as_object()
        .and_then(|m| m.get("_conversation_id"))
        .and_then(|v| v.as_str())
}

/// Register all computer-use tools (mouse, hotkey, composite_action, modified_click, wait, clipboard).
pub fn register_all(reg: &ToolRegistry, state: Arc<ComputerState>) {
    let mouse_state = state.clone();
    let mouse_doc = PRIMARY_MOUSE_DOC.trim();
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
            let tier = mouse_state.tier_for_conversation(&cid);
            let method = args["method"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'method' parameter"))?
                .to_string();
            ensure_method_allowed_for_tier(tier, &method)?;
            run_synthetic_computer_tool(tier, move || {
                ensure_index_method_allowed(&method)?;
                let vision = mouse_state.vision_state_for_conversation(&cid);
                let hl_default = effective_human_like_default();
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
    let composite_doc = PRIMARY_COMPOSITE_DOC.trim();
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
            let tier = composite_state.tier_for_conversation(&cid);
            let method = args["method"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'method' parameter"))?
                .to_string();
            ensure_method_allowed_for_tier(tier, &method)?;
            run_synthetic_computer_tool(tier, move || {
                ensure_index_method_allowed(&method)?;
                let vision = composite_state.vision_state_for_conversation(&cid);
                let hl_default = effective_human_like_default();
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
    let modified_doc = PRIMARY_MODIFIED_CLICK_DOC.trim();
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
            let tier = modified_state.tier_for_conversation(&cid);
            let method = args["method"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'method' parameter"))?
                .to_string();
            ensure_method_allowed_for_tier(tier, &method)?;
            run_synthetic_computer_tool(tier, move || {
                ensure_index_method_allowed(&method)?;
                let vision = modified_state.vision_state_for_conversation(&cid);
                let hl_default = effective_human_like_default();
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

    let tier_signal_doc = include_str!("prompts/sidecar/tier_signal.md").trim();
    reg.register(ToolEntry::new_sidecar(
        "verify",
        "low",
        false,
        tier_signal_doc,
        Arc::new(move |args| tool_tier_signal::execute_tier_signal(&args)),
    ));
}
