//! Computer-use **tool handlers** registered with [`crate::tools::ToolRegistry`].
//!
//! Sibling modules (`actions`, `screen`, [`crate::agents::computer::ComputerState`], etc.) live in the parent [`crate::agents::computer`] package.

pub mod args_util;
pub mod method_route;
pub(crate) mod tool_app_access;
mod tool_clipboard;
mod tool_hotkey;
pub(crate) mod tool_input;
mod tool_keyboard;
pub(crate) mod tool_modified_click;
pub(crate) mod tool_mouse;
mod tool_wait;

pub use tool_app_access::{
    app_access_host_pass_summary, parse_app_access_host_outcome, AppAccessHostOutcome,
};
pub use tool_clipboard::{clipboard_host_verify_kind, ClipboardHostVerifyKind};

use crate::agents::computer::ComputerState;
use crate::platform::run_synthetic_input;
use crate::tools::tool_doc::load_tools_from_schema_yaml;
use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
pub(crate) use method_route::{InputBackend, ModifiedClickBackend, MouseBackend};
use std::collections::HashMap;
use std::sync::Arc;
use tool_input::InputTool;

/// Run synthetic input on the platform main thread when required.
fn run_synthetic_computer_tool<R, F>(f: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    run_synthetic_input(f)
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
            ("mouse_click_at", Some((MouseBackend::At, "click"))),
            (
                "mouse_double_click_at",
                Some((MouseBackend::At, "double_click")),
            ),
            (
                "mouse_right_click_at",
                Some((MouseBackend::At, "right_click")),
            ),
            ("mouse_hover_at", Some((MouseBackend::At, "hover"))),
            // `target_desc` is optional here, but the sync handler cannot ground
            // and cannot move the pointer — so this must go through the async
            // dispatch path even when the caller omits a target.
            (
                "mouse_scroll_current",
                Some((MouseBackend::At, "scroll_current")),
            ),
            (
                "mouse_drag_from_to_at",
                Some((MouseBackend::At, "drag_from_to")),
            ),
            ("mouse_down", Some((MouseBackend::At, "mouse_down"))),
            ("mouse_up", Some((MouseBackend::At, "mouse_up"))),
        ];

        for (name, backend_method) in mouse_handlers {
            let ms = state.clone();
            let prompt = doc.clone();
            let tool_name = name.to_string();
            let backend_method = *backend_method;

            let err_name = tool_name.clone();
            let handler: ToolHandler = Arc::new(move |args| {
                let cid = conversation_id_from_args(&args)
                    .unwrap_or_default()
                    .to_string();

                if let Some((_backend, _method)) = backend_method {
                    // These run on the chat runtime async path: they accept a
                    // `description` and may have to ground first, and grounding
                    // needs the agent's provider — which this closure does not
                    // carry. See dispatch/mouse.rs.
                    let _ = (&ms, &cid);
                    Err(anyhow::anyhow!(
                        "{} is executed by the chat runtime async path, not synchronous invoke",
                        err_name
                    ))
                } else {
                    // Unreachable in practice: every mouse name carries a
                    // `backend_method`, so this branch never runs. Kept as a
                    // fallback. Note that scrolling takes `notches_x` /
                    // `notches_y` — wheel notches, not pixel deltas.
                    let control = ms.control.clone();
                    run_synthetic_computer_tool(move || {
                        tool_mouse::scroll_current(&control, &args)
                    })
                }
            });

            let schema = mouse_schemas
                .get(*name)
                .cloned()
                .unwrap_or_else(|| panic!("mouse.schema.yaml missing entry for {name}"));

            reg.register(
                ToolEntry::new(tool_name, MOUSE_DOC_SOURCE, "low", false, prompt, handler)
                    .with_schema(schema)
                    .with_subagent_inheritance(false),
            );
        }
    }

    // ── hotkey ───────────────────────────────────────────────────────────
    {
        const HOTKEY_DOC_SOURCE: &str = "agents/computer/tools/prompts/hotkey.md";
        let hotkey_state = state.clone();
        let doc = include_str!("prompts/hotkey.md").trim();
        reg.register(
            ToolEntry::new(
                "hotkey",
                HOTKEY_DOC_SOURCE,
                "low",
                false,
                doc,
                Arc::new(move |args| {
                    let hotkey_state = hotkey_state.clone();
                    run_synthetic_input(move || {
                        let tool = tool_hotkey::HotkeyTool::new(hotkey_state.control.clone());
                        tool.execute("hotkey", &args)
                    })
                }),
            )
            .with_subagent_inheritance(false),
        );
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

            let err_name = tool_name.clone();
            let handler: ToolHandler = Arc::new(move |args| {
                let cid = conversation_id_from_args(&args)
                    .unwrap_or_default()
                    .to_string();
                match backend {
                    // input_at takes {target_desc, bbox} and grounds first, which
                    // needs the agent's provider — see dispatch/input.rs.
                    InputBackend::At => {
                        let _ = (&st, &cid);
                        Err(anyhow::anyhow!(
                            "{} is executed by the chat runtime async path, not synchronous invoke",
                            err_name
                        ))
                    }
                    InputBackend::Focused => {
                        let vision = st.vision_state_for_conversation(&cid);
                        let tool = InputTool::new(st.control.clone(), vision);
                        run_synthetic_computer_tool(move || tool.execute_with(backend, &args))
                    }
                }
            });

            reg.register(
                ToolEntry::new(tool_name, INPUT_DOC_SOURCE, "low", false, prompt, handler)
                    .with_schema(schema)
                    .with_subagent_inheritance(false),
            );
        }
    }

    // ── keyboard (flat tools: PRESS / KEY_DOWN / KEY_UP / TYPING) ─────────
    {
        const KEYBOARD_DOC_SOURCE: &str = "agents/computer/tools/prompts/keyboard.md";
        const KEYBOARD_SCHEMA_YAML: &str = include_str!("prompts/keyboard.schema.yaml");
        let keyboard_schemas: HashMap<String, serde_json::Value> =
            load_tools_from_schema_yaml(KEYBOARD_SCHEMA_YAML)
                .expect("keyboard.schema.yaml must be valid")
                .into_iter()
                .collect();

        let doc = include_str!("prompts/keyboard.md").trim().to_string();
        let keyboard_handlers: &[&str] =
            &["key_press", "key_down", "key_up", "keyboard_type"];

        for name in keyboard_handlers {
            let st = state.clone();
            let prompt = doc.clone();
            let tool_name = name.to_string();
            let method = name.to_string();

            let handler: ToolHandler = Arc::new(move |args| {
                let tool = tool_keyboard::KeyboardTool::new(st.control.clone());
                let method = method.clone();
                run_synthetic_computer_tool(move || tool.execute(&method, &args))
            });

            let schema = keyboard_schemas
                .get(*name)
                .cloned()
                .unwrap_or_else(|| panic!("keyboard.schema.yaml missing entry for {name}"));

            reg.register(
                ToolEntry::new(
                    tool_name,
                    KEYBOARD_DOC_SOURCE,
                    "low",
                    false,
                    prompt,
                    handler,
                )
                .with_schema(schema)
                .with_subagent_inheritance(false),
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
            (
                "modified_click_select_at",
                ModifiedClickBackend::At,
                "select",
            ),
            (
                "modified_click_range_select_at",
                ModifiedClickBackend::At,
                "range_select",
            ),
        ];

        for (name, backend, method) in mc_handlers {
            let st = state.clone();
            let prompt = doc.clone();
            let tool_name = name.to_string();
            let backend = *backend;
            let method = method.to_string();

            let err_name = tool_name.clone();
            let handler: ToolHandler = Arc::new(move |args| {
                let cid = conversation_id_from_args(&args)
                    .unwrap_or_default()
                    .to_string();
                // positions are {target_desc, bbox} now and each one has to be
                // grounded first — see dispatch/modified_click.rs.
                let _ = (&st, &cid, &method, &backend);
                Err(anyhow::anyhow!(
                    "{} is executed by the chat runtime async path, not synchronous invoke",
                    err_name
                ))
            });

            let schema = modified_click_schemas
                .get(*name)
                .cloned()
                .unwrap_or_else(|| panic!("modified_click.schema.yaml missing entry for {name}"));

            reg.register(
                ToolEntry::new(
                    tool_name,
                    MODIFIED_CLICK_DOC_SOURCE,
                    "low",
                    false,
                    prompt,
                    handler,
                )
                .with_schema(schema)
                .with_subagent_inheritance(false),
            );
        }
    }

    // ── wait ─────────────────────────────────────────────────────────────
    {
        const WAIT_DOC_SOURCE: &str = "agents/computer/tools/prompts/wait.md";
        let doc = include_str!("prompts/wait.md").trim();
        reg.register(
            ToolEntry::new(
                "wait",
                WAIT_DOC_SOURCE,
                "low",
                false,
                doc,
                Arc::new(move |args| {
                    let tool = tool_wait::WaitTool::new();
                    tool.execute("wait", &args)
                }),
            )
            .with_subagent_inheritance(false),
        );
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
        let cb_handlers: &[(&str, &str)] =
            &[("clipboard_read", "read"), ("clipboard_write", "write")];

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
                ToolEntry::new(
                    tool_name,
                    CLIPBOARD_DOC_SOURCE,
                    "low",
                    false,
                    prompt,
                    handler,
                )
                .with_schema(schema)
                .with_subagent_inheritance(false),
            );
        }
    }

    // ── list_apps / launch_app (Codex Computer Use aligned) ───────────────
    {
        const LIST_APPS_DOC_SOURCE: &str = "agents/computer/tools/prompts/list_apps.md";
        const LAUNCH_APP_DOC_SOURCE: &str = "agents/computer/tools/prompts/launch_app.md";
        const LIST_APPS_SCHEMA_YAML: &str = include_str!("prompts/list_apps.schema.yaml");
        const LAUNCH_APP_SCHEMA_YAML: &str = include_str!("prompts/launch_app.schema.yaml");
        let list_schema = load_tools_from_schema_yaml(LIST_APPS_SCHEMA_YAML)
            .expect("list_apps.schema.yaml must be valid")
            .into_iter()
            .next()
            .expect("list_apps.schema.yaml missing list_apps entry")
            .1;
        let launch_schema = load_tools_from_schema_yaml(LAUNCH_APP_SCHEMA_YAML)
            .expect("launch_app.schema.yaml must be valid")
            .into_iter()
            .next()
            .expect("launch_app.schema.yaml missing launch_app entry")
            .1;

        let list_doc = include_str!("prompts/list_apps.md").trim();
        reg.register(
            ToolEntry::new(
                "list_apps",
                LIST_APPS_DOC_SOURCE,
                "low",
                false,
                list_doc,
                Arc::new(move |args| {
                    tool_app_access::AppAccessTool::new().execute_list_apps(&args)
                }),
            )
            .with_schema(list_schema)
            .with_subagent_inheritance(false),
        );

        let launch_doc = include_str!("prompts/launch_app.md").trim();
        reg.register(
            ToolEntry::new(
                "launch_app",
                LAUNCH_APP_DOC_SOURCE,
                "low",
                false,
                launch_doc,
                Arc::new(move |args| {
                    tool_app_access::AppAccessTool::new().execute_launch_app(&args)
                }),
            )
            .with_schema(launch_schema)
            .with_subagent_inheritance(false),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::parallel::ToolConflictClass;
    use crate::tools::ToolRegistry;
    use std::sync::Arc;

    fn registry_with_computer_tools() -> ToolRegistry {
        let reg = ToolRegistry::new();
        let state = Arc::new(ComputerState::default_for_test());
        register_all(&reg, state);
        reg
    }

    #[test]
    fn locate_element_is_not_registered() {
        // Grounding is internal now: pointing tools locate the target themselves,
        // so there is no standalone tool for the model to call first.
        let reg = registry_with_computer_tools();
        assert!(!reg.list_defs().iter().any(|d| d.name == "locate_element"));
        assert!(reg.openai_tools(&["locate_element".to_string()]).is_empty());
    }

    /// The six tools P4 adds on top of the existing surface (design §7.4).
    ///
    /// `mouse_drag_to` (OSWorld `DRAG_TO`) was dropped: its grab point is the
    /// current pointer, which the model cannot set. `mouse_drag_from_to_at`
    /// covers the same gesture with both ends grounded.
    const NEW_ACTION_TOOLS: &[&str] = &[
        "mouse_down",
        "mouse_up",
        "key_press",
        "key_down",
        "key_up",
        "keyboard_type",
    ];

    #[test]
    fn registers_the_new_action_tools_with_schemas() {
        let reg = registry_with_computer_tools();
        let names: Vec<String> = reg.list_defs().iter().map(|d| d.name.clone()).collect();
        for tool in NEW_ACTION_TOOLS {
            assert!(names.iter().any(|n| n == tool), "{tool} is not registered");
        }

        // Each new tool must reach the LLM with its own schema, through the
        // family allow entries the computer agent actually uses.
        let allow = vec!["mouse".to_string(), "keyboard".to_string()];
        let exposed = reg.openai_tools(&allow);
        let exposed_names: Vec<&str> = exposed
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        for tool in NEW_ACTION_TOOLS {
            assert!(
                exposed_names.contains(tool),
                "{tool} missing from openai_tools: {exposed_names:?}"
            );
        }
    }

    #[test]
    fn new_action_tools_are_serial_computer_tools() {
        let reg = registry_with_computer_tools();
        for tool in NEW_ACTION_TOOLS {
            assert!(!reg.is_parallel_eligible(tool), "{tool} must be serial");
            assert_eq!(
                reg.tool_conflict_class(tool),
                ToolConflictClass::Computer,
                "{tool} must be classified as a Computer tool"
            );
        }
    }

    #[test]
    fn mouse_click_at_schema_exposes_button_and_clicks() {
        let reg = registry_with_computer_tools();
        let tools = reg.openai_tools(&["mouse_click_at".to_string()]);
        assert_eq!(tools.len(), 1);
        let props = &tools[0]["function"]["parameters"]["properties"];
        assert_eq!(
            props["button"]["enum"],
            serde_json::json!(["left", "middle", "right"])
        );
        assert_eq!(props["clicks"]["maximum"], 2);
        assert_eq!(props["clicks"]["minimum"], 1);
    }

    #[test]
    fn mouse_scroll_current_schema_is_notch_based() {
        let reg = registry_with_computer_tools();
        let tools = reg.openai_tools(&["mouse_scroll_current".to_string()]);
        assert_eq!(tools.len(), 1);
        let params = &tools[0]["function"]["parameters"];
        let props = &params["properties"];
        assert!(props.get("notches_x").is_some(), "{props}");
        assert!(props.get("notches_y").is_some(), "{props}");
        // The pixel-era names are gone on purpose: keeping them as aliases would
        // let the model keep sending `-900` forever.
        assert!(props.get("dx").is_none(), "{props}");
        assert!(props.get("dy").is_none(), "{props}");
        assert!(
            props.get("lines").is_none(),
            "line units were replaced by notches: {props}"
        );
        assert_eq!(params["required"], serde_json::json!(["goal"]));
    }
}
