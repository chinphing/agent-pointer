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
use args_util::{clamp_scroll_lines, effective_human_like_default};
use crate::platform::run_synthetic_input;
use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
use method_route::{CompositeBackend, ModifiedClickBackend, MouseBackend};
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
    // ── mouse (flat tools) ────────────────────────────────────────────────
    {
        let doc = include_str!("prompts/mouse.md").trim().to_string();
        let mouse_handlers: &[(&str, Option<(MouseBackend, &str)>)] = &[
            ("mouse_click_index", Some((MouseBackend::Index, "click"))),
            ("mouse_click_at", Some((MouseBackend::At, "click"))),
            ("mouse_double_click_index", Some((MouseBackend::Index, "double_click"))),
            ("mouse_double_click_at", Some((MouseBackend::At, "double_click"))),
            ("mouse_right_click_index", Some((MouseBackend::Index, "right_click"))),
            ("mouse_right_click_at", Some((MouseBackend::At, "right_click"))),
            ("mouse_hover_index", Some((MouseBackend::Index, "hover"))),
            ("mouse_hover_at", Some((MouseBackend::At, "hover"))),
            ("mouse_scroll_current", None),
            ("mouse_scroll_index", Some((MouseBackend::Index, "scroll"))),
            ("mouse_drag_from_to_index", Some((MouseBackend::Index, "drag_from_to"))),
            ("mouse_drag_from_to_at", Some((MouseBackend::At, "drag_from_to"))),
        ];

        for (name, backend_method) in mouse_handlers {
            let ms = state.clone();
            let prompt = doc.clone();
            let tool_name = name.to_string();
            let backend_method = *backend_method;

            let handler: ToolHandler = Arc::new(move |args| {
                let cid = conversation_id_from_args(&args)
                    .unwrap_or_default()
                    .to_string();
                let tier = ms.tier_for_conversation(&cid);

                if let Some((backend, method)) = backend_method {
                    let vision = ms.vision_state_for_conversation(&cid);
                    let hl_default = effective_human_like_default();
                    let tool =
                        MouseTool::new(ms.executor.clone(), vision, hl_default);
                    run_synthetic_computer_tool(tier, move || {
                        tool.execute_with(backend, method, &args)
                    })
                } else {
                    // mouse_scroll
                    let lines_raw = args["lines"]
                        .as_i64()
                        .ok_or_else(|| anyhow::anyhow!("Missing 'lines' parameter"))?
                        as i32;
                    let lines = clamp_scroll_lines(lines_raw)?;
                    let executor = ms.executor.clone();
                    run_synthetic_computer_tool(tier, move || {
                        executor
                            .lock()
                            .unwrap()
                            .scroll_at_current(lines)
                            .map(|_| format!("Scrolled {} lines", lines))
                            .map_err(|e| anyhow::anyhow!("{}", e))
                    })
                }
            });

            reg.register(ToolEntry::new(
                tool_name,
                "low",
                false,
                prompt,
                handler,
            ));
        }
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

    // ── input (flat tools) ─────────────────────────────────────────────
    {
        let doc = include_str!("prompts/input.md").trim().to_string();
        let input_handlers: &[(&str, CompositeBackend, &str)] = &[
            ("input_index", CompositeBackend::Index, "type_text"),
            ("input_at", CompositeBackend::At, "type_text"),
            ("input_focused", CompositeBackend::Focused, "type_text"),
        ];

        for (name, backend, method) in input_handlers {
            let st = state.clone();
            let prompt = doc.clone();
            let tool_name = name.to_string();
            let backend = *backend;
            let method = method.to_string();

            let handler: ToolHandler = Arc::new(move |args| {
                let cid = conversation_id_from_args(&args)
                    .unwrap_or_default()
                    .to_string();
                let tier = st.tier_for_conversation(&cid);
                let vision = st.vision_state_for_conversation(&cid);
                let hl_default = effective_human_like_default();
                let tool = CompositeActionTool::new(st.executor.clone(), vision, hl_default);
                run_synthetic_computer_tool(tier, {
                    let method = method.clone();
                    move || {
                        tool.execute_with(backend, &method, &args)
                    }
                })
            });

            reg.register(ToolEntry::new(
                tool_name,
                "low",
                false,
                prompt,
                handler,
            ));
        }
    }

    // ── modified_click (flat tools) ─────────────────────────────────────────
    {
        let doc = include_str!("prompts/modified_click.md").trim().to_string();
        let mc_handlers: &[(&str, ModifiedClickBackend, &str)] = &[
            ("modified_click_select_index", ModifiedClickBackend::Index, "select"),
            ("modified_click_range_select_index", ModifiedClickBackend::Index, "range_select"),
            ("modified_click_select_at", ModifiedClickBackend::At, "select"),
            ("modified_click_range_select_at", ModifiedClickBackend::At, "range_select"),
        ];

        for (name, backend, method) in mc_handlers {
            let st = state.clone();
            let prompt = doc.clone();
            let tool_name = name.to_string();
            let backend = *backend;
            let method = method.to_string();

            let handler: ToolHandler = Arc::new(move |args| {
                let cid = conversation_id_from_args(&args)
                    .unwrap_or_default()
                    .to_string();
                let tier = st.tier_for_conversation(&cid);
                let vision = st.vision_state_for_conversation(&cid);
                let hl_default = effective_human_like_default();
                let tool = ModifiedClickTool::new(st.executor.clone(), vision, hl_default);
                run_synthetic_computer_tool(tier, {
                    let method = method.clone();
                    move || {
                        tool.execute_with(backend, &method, &args)
                    }
                })
            });

            reg.register(ToolEntry::new(
                tool_name,
                "low",
                false,
                prompt,
                handler,
            ));
        }
    }

    // ── captcha_verify (flat tools) ───────────────────────────────────────
    {
        let cv_state = state.clone();
        let doc = include_str!("prompts/captcha_verify.md").trim().to_string();
        let cv_handlers: &[(&str, &str)] = &[
            ("captcha_verify_type", "type"),
            ("captcha_verify_click", "click"),
            ("captcha_verify_drag", "drag"),
        ];

        for (name, method) in cv_handlers {
            let s = cv_state.clone();
            let prompt = doc.clone();
            let tool_name = name.to_string();
            let method = method.to_string();

            let handler: ToolHandler = Arc::new(move |args| {
                let cid = conversation_id_from_args(&args)
                    .unwrap_or_default()
                    .to_string();
                let tier = s.tier_for_conversation(&cid);
                let vision = s.vision_state_for_conversation(&cid);
                let tool = tool_captcha_verify::CaptchaVerifyTool::new(
                    s.executor.clone(),
                    tier,
                    s.clone(),
                    cid.clone(),
                    vision,
                );
                tool.execute(&method, &args)
            });

            reg.register(ToolEntry::new(
                tool_name,
                "low",
                false,
                prompt,
                handler,
            ));
        }
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

    // ── clipboard (flat tools) ────────────────────────────────────────────
    {
        let doc = include_str!("prompts/clipboard.md").trim().to_string();
        let cb_handlers: &[(&str, &str)] = &[
            ("clipboard_read", "read"),
            ("clipboard_write", "write"),
        ];

        for (name, method) in cb_handlers {
            let prompt = doc.clone();
            let tool_name = name.to_string();
            let method = method.to_string();

            let handler: ToolHandler = Arc::new(move |args| {
                let tool = tool_clipboard::ClipboardTool::new();
                tool.execute(&method, &args)
            });

            reg.register(ToolEntry::new(
                tool_name,
                "low",
                false,
                prompt,
                handler,
            ));
        }
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
