//! Computer-use **tool handlers** registered with [`crate::tools::ToolRegistry`].
//!
//! Sibling modules (`actions`, `screen`, [`crate::agents::computer::ComputerState`], etc.) live in the parent [`crate::agents::computer`] package.

pub mod args_util;
mod dati_client;
pub mod method_route;
mod tool_captcha_verify;
mod tool_clipboard;
mod tool_input;
mod tool_hotkey;
mod tool_modified_click;
mod tool_mouse;
mod tool_action_verify;
mod tool_wait;

use crate::agents::computer::tool_names::ACTION_VERIFY;
use crate::agents::computer::ComputerState;
use crate::agents::computer::tier::ComputerTierGuard;
use args_util::{clamp_scroll_lines, effective_human_like_default};
use crate::platform::run_synthetic_input;
use crate::tools::tool_doc::load_tools_from_schema_yaml;
use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
use method_route::{InputBackend, ModifiedClickBackend, MouseBackend};
use std::collections::HashMap;
use std::sync::Arc;
use tool_input::InputTool;
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
        const MOUSE_DOC_SOURCE: &str = "agents/computer/tools/prompts/mouse.md";
        const MOUSE_SCHEMA_YAML: &str = include_str!("prompts/mouse.schema.yaml");
        let mouse_schemas: HashMap<String, serde_json::Value> =
            load_tools_from_schema_yaml(MOUSE_SCHEMA_YAML)
                .expect("mouse.schema.yaml must be valid")
                .into_iter()
                .collect();

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

            let schema = mouse_schemas
                .get(*name)
                .cloned()
                .unwrap_or_else(|| panic!("mouse.schema.yaml missing entry for {name}"));

            reg.register(
                ToolEntry::new(tool_name, MOUSE_DOC_SOURCE, "low", false, prompt, handler)
                    .with_schema(schema),
            );
        }
    }

    // ── hotkey ───────────────────────────────────────────────────────────
    {
        const HOTKEY_DOC_SOURCE: &str = "agents/computer/tools/prompts/hotkey.md";
        let hotkey_state = state.clone();
        let doc = include_str!("prompts/hotkey.md").trim();
        reg.register(ToolEntry::new(
            "hotkey",
            HOTKEY_DOC_SOURCE,
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
        const INPUT_DOC_SOURCE: &str = "agents/computer/tools/prompts/input.md";
        const INPUT_SCHEMA_YAML: &str = include_str!("prompts/input.schema.yaml");
        let input_schemas: HashMap<String, serde_json::Value> =
            load_tools_from_schema_yaml(INPUT_SCHEMA_YAML)
                .expect("input.schema.yaml must be valid")
                .into_iter()
                .collect();

        let doc = include_str!("prompts/input.md").trim().to_string();
        let input_handlers: &[(&str, InputBackend)] = &[
            ("input_index", InputBackend::Index),
            ("input_at", InputBackend::At),
            ("input_focused", InputBackend::Focused),
        ];

        for (name, backend) in input_handlers {
            let st = state.clone();
            let prompt = doc.clone();
            let tool_name = name.to_string();
            let backend = *backend;
            let schema = input_schemas
                .get(*name)
                .cloned()
                .unwrap_or_else(|| panic!("input.schema.yaml missing entry for {name}"));

            let handler: ToolHandler = Arc::new(move |args| {
                let cid = conversation_id_from_args(&args)
                    .unwrap_or_default()
                    .to_string();
                let tier = st.tier_for_conversation(&cid);
                let vision = st.vision_state_for_conversation(&cid);
                let hl_default = effective_human_like_default();
                let tool = InputTool::new(st.executor.clone(), vision, hl_default);
                run_synthetic_computer_tool(tier, move || tool.execute_with(backend, &args))
            });

            reg.register(
                ToolEntry::new(tool_name, INPUT_DOC_SOURCE, "low", false, prompt, handler)
                    .with_schema(schema),
            );
        }
    }

    // ── modified_click (flat tools) ─────────────────────────────────────────
    {
        const MODIFIED_CLICK_DOC_SOURCE: &str = "agents/computer/tools/prompts/modified_click.md";
        const MODIFIED_CLICK_SCHEMA_YAML: &str = include_str!("prompts/modified_click.schema.yaml");
        let modified_click_schemas: HashMap<String, serde_json::Value> =
            load_tools_from_schema_yaml(MODIFIED_CLICK_SCHEMA_YAML)
                .expect("modified_click.schema.yaml must be valid")
                .into_iter()
                .collect();

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

            let schema = modified_click_schemas
                .get(*name)
                .cloned()
                .unwrap_or_else(|| panic!("modified_click.schema.yaml missing entry for {name}"));

            reg.register(
                ToolEntry::new(tool_name, MODIFIED_CLICK_DOC_SOURCE, "low", false, prompt, handler)
                    .with_schema(schema),
            );
        }
    }

    // ── captcha_verify (flat tools) ───────────────────────────────────────
    {
        const CAPTCHA_DOC_SOURCE: &str = "agents/computer/tools/prompts/captcha_verify.md";
        const CAPTCHA_SCHEMA_YAML: &str = include_str!("prompts/captcha_verify.schema.yaml");
        let captcha_schemas: HashMap<String, serde_json::Value> =
            load_tools_from_schema_yaml(CAPTCHA_SCHEMA_YAML)
                .expect("captcha_verify.schema.yaml must be valid")
                .into_iter()
                .collect();

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
            let schema = captcha_schemas
                .get(*name)
                .cloned()
                .unwrap_or_else(|| panic!("captcha_verify.schema.yaml missing entry for {name}"));

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

            reg.register(
                ToolEntry::new(tool_name, CAPTCHA_DOC_SOURCE, "low", false, prompt, handler)
                    .with_schema(schema),
            );
        }
    }

    // ── wait ─────────────────────────────────────────────────────────────
    {
        const WAIT_DOC_SOURCE: &str = "agents/computer/tools/prompts/wait.md";
        let doc = include_str!("prompts/wait.md").trim();
        reg.register(ToolEntry::new(
            "wait",
            WAIT_DOC_SOURCE,
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
        const CLIPBOARD_DOC_SOURCE: &str = "agents/computer/tools/prompts/clipboard.md";
        const CLIPBOARD_SCHEMA_YAML: &str = include_str!("prompts/clipboard.schema.yaml");
        let clipboard_schemas: HashMap<String, serde_json::Value> =
            load_tools_from_schema_yaml(CLIPBOARD_SCHEMA_YAML)
                .expect("clipboard.schema.yaml must be valid")
                .into_iter()
                .collect();

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

            let schema = clipboard_schemas
                .get(*name)
                .cloned()
                .unwrap_or_else(|| panic!("clipboard.schema.yaml missing entry for {name}"));

            reg.register(
                ToolEntry::new(tool_name, CLIPBOARD_DOC_SOURCE, "low", false, prompt, handler)
                    .with_schema(schema),
            );
        }
    }

    // ── action_verify (sidecar flat) ─────────────────────────────────────
    {
        const ACTION_VERIFY_DOC_SOURCE: &str = "agents/computer/tools/prompts/action_verify.md";
        let doc = include_str!("prompts/action_verify.md").trim();
        reg.register(ToolEntry::new_sidecar(
            ACTION_VERIFY,
            ACTION_VERIFY_DOC_SOURCE,
            "low",
            false,
            doc,
            Arc::new(move |args| tool_action_verify::execute_action_verify(&args)),
        ));
    }
}
