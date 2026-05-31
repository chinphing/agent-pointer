//! Computer-use **tool handlers** registered with [`crate::tools::ToolRegistry`].
//!
//! Sibling modules (`actions`, `screen`, [`crate::agents::computer::ComputerState`], etc.) live in the parent [`crate::agents::computer`] package.

pub mod args_util;
mod dati_client;
pub mod method_route;
mod tool_captcha_verify;
mod tool_clipboard;
mod tool_composite;
mod tool_hotkey;
mod tool_modified_click;
mod tool_mouse;
mod tool_tier_signal;
mod tool_wait;

use crate::agents::computer::ComputerState;
use crate::agents::computer::tier::ComputerTierGuard;
use args_util::effective_human_like_default;
use crate::platform::run_synthetic_input;
use crate::tools::{ToolEntry, ToolRegistry};
use std::sync::Arc;
use tool_composite::CompositeActionTool;
use tool_modified_click::ModifiedClickTool;
use tool_mouse::MouseTool;

/// Run synthetic input on the platform main thread when required, with tier context set there.
fn run_synthetic_computer_tool<R, F>(tier: crate::agents::computer::tier::ComputerTier, f: F) -> R
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

/// Register all computer-use tools.
pub fn register_all(reg: &ToolRegistry, state: Arc<ComputerState>) {
    // ── mouse (unified: index / at / current via method name) ────────────
    {
        let mouse_state = state.clone();
        let doc = include_str!("prompts/mouse.md").trim();
        reg.register(ToolEntry::new(
            "mouse",
            "low",
            false,
            doc,
            Arc::new(move |args| {
                let mouse_state = mouse_state.clone();
                let cid = conversation_id_from_args(&args)
                    .unwrap_or_default()
                    .to_string();
                let tier = mouse_state.tier_for_conversation(&cid);
                let vision = mouse_state.vision_state_for_conversation(&cid);
                let hl_default = effective_human_like_default();
                let tool = MouseTool::new(
                    mouse_state.executor.clone(),
                    vision,
                    hl_default,
                );
                run_synthetic_computer_tool(tier, move || tool.execute(&args))
            }),
        ));
    }

    // ── hotkey ───────────────────────────────────────────────────────────
    {
        let hotkey_state = state.clone();
        let doc = include_str!("prompts/hotkey.md").trim();
        reg.register(ToolEntry::new(
            "hotkey",
            "low",
            false,
            doc,
            Arc::new(move |args| {
                let hotkey_state = hotkey_state.clone();
                run_synthetic_input(move || {
                    let tool = tool_hotkey::HotkeyTool::new(hotkey_state.executor.clone());
                    tool.execute("hotkey", &args)
                })
            }),
        ));
    }

    // ── composite_action (unified) ───────────────────────────────────────
    {
        let st = state.clone();
        let doc = include_str!("prompts/composite_action.md").trim();
        reg.register(ToolEntry::new(
            "composite_action",
            "low",
            false,
            doc,
            Arc::new(move |args| {
                let st = st.clone();
                let cid = conversation_id_from_args(&args)
                    .unwrap_or_default()
                    .to_string();
                let tier = st.tier_for_conversation(&cid);
                let vision = st.vision_state_for_conversation(&cid);
                let hl_default = effective_human_like_default();
                let tool = CompositeActionTool::new(st.executor.clone(), vision, hl_default);
                run_synthetic_computer_tool(tier, move || tool.execute(&args))
            }),
        ));
    }

    // ── modified_click (unified) ─────────────────────────────────────────
    {
        let st = state.clone();
        let doc = include_str!("prompts/modified_click.md").trim();
        reg.register(ToolEntry::new(
            "modified_click",
            "low",
            false,
            doc,
            Arc::new(move |args| {
                let st = st.clone();
                let cid = conversation_id_from_args(&args)
                    .unwrap_or_default()
                    .to_string();
                let tier = st.tier_for_conversation(&cid);
                let vision = st.vision_state_for_conversation(&cid);
                let hl_default = effective_human_like_default();
                let tool = ModifiedClickTool::new(st.executor.clone(), vision, hl_default);
                run_synthetic_computer_tool(tier, move || tool.execute(&args))
            }),
        ));
    }

    // ── captcha_verify ───────────────────────────────────────────────────
    {
        let captcha_state = state.clone();
        let doc = include_str!("prompts/captcha_verify.md").trim();
        reg.register(ToolEntry::new(
            "captcha_verify",
            "low",
            false,
            doc,
            Arc::new(move |args| {
                let captcha_state = captcha_state.clone();
                let cid = conversation_id_from_args(&args)
                    .unwrap_or_default()
                    .to_string();
                let tier = captcha_state.tier_for_conversation(&cid);
                let method = args
                    .get("action")
                    .and_then(|v| v.as_str())
                    .or_else(|| args.get("method").and_then(|v| v.as_str()))
                    .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?
                    .to_string();
                let vision = captcha_state.vision_state_for_conversation(&cid);
                let tool = tool_captcha_verify::CaptchaVerifyTool::new(
                    captcha_state.executor.clone(),
                    tier,
                    captcha_state.clone(),
                    cid.clone(),
                    vision,
                );
                tool.execute(&method, &args)
            }),
        ));
    }

    // ── wait ─────────────────────────────────────────────────────────────
    {
        let doc = include_str!("prompts/wait.md").trim();
        reg.register(ToolEntry::new(
            "wait",
            "low",
            false,
            doc,
            Arc::new(move |args| {
                let tool = tool_wait::WaitTool::new();
                tool.execute("wait", &args)
            }),
        ));
    }

    // ── clipboard ────────────────────────────────────────────────────────
    {
        let doc = include_str!("prompts/clipboard.md").trim();
        reg.register(ToolEntry::new(
            "clipboard",
            "low",
            false,
            doc,
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

    // ── verify (sidecar) ─────────────────────────────────────────────────
    {
        let doc = include_str!("prompts/sidecar/tier_signal.md").trim();
        reg.register(ToolEntry::new_sidecar(
            "verify",
            "low",
            false,
            doc,
            Arc::new(move |args| tool_tier_signal::execute_tier_signal(&args)),
        ));
    }
}
