pub mod action_enigo;
pub mod actions;
pub mod annotate;
pub mod args_util;
pub mod coord;
pub mod screen;
pub mod tool_composite;
pub mod tool_hotkey;
pub mod tool_mouse;
mod tool_modified_click;
pub mod tool_wait;
pub mod verify;
pub mod vision_state;

use crate::agents::AgentRegistry;
use crate::tools::{ToolEntry, ToolRegistry};
use tool_modified_click::ModifiedClickTool;
use actions::ActionExecutor;
use annotate::AnnotateClient;
use std::sync::{Arc, Mutex};
use vision_state::VisionState;

/// Default annotation service URL.
const DEFAULT_ANNOTATE_API_BASE: &str = "http://127.0.0.1:8000";
/// Config key for the annotation service URL in Computer Agent's config.
const CONFIG_KEY_ANNOTATE_API_BASE: &str = "annotateApiBase";

/// Shared state for computer use tools.
///
/// This struct is created once in AppState and shared across all computer tool handlers.
/// It holds the action executor, vision state, and annotation client for the current session.
#[derive(Debug)]
pub struct ComputerState {
    /// The action executor.
    pub executor: Arc<Mutex<ActionExecutor>>,
    /// The current vision state.
    pub vision_state: Arc<Mutex<VisionState>>,
    /// The annotation service client.
    pub annotate_client: AnnotateClient,
}

impl ComputerState {
    /// Create a new ComputerState, loading the annotation URL from the Computer Agent's config.
    ///
    /// # Arguments
    /// * `agents` - The agent registry to look up the Computer Agent's config.
    ///
    /// The backend will be initialized with the enigo implementation.
    /// If enigo fails to initialize, tools will return errors at runtime.
    pub fn new(agents: &AgentRegistry) -> Self {
        let annotate_api_base = agents
            .get("computer")
            .map(|agent| agent.def())
            .and_then(|def| def.config.get(CONFIG_KEY_ANNOTATE_API_BASE).cloned())
            .unwrap_or_else(|| DEFAULT_ANNOTATE_API_BASE.to_string());
        Self::with_annotate_url(&annotate_api_base)
    }

    /// Create a new ComputerState with an explicit annotation service URL.
    ///
    /// # Arguments
    /// * `annotate_api_base` - Base URL for the annotation service. If empty, uses the default.
    pub fn with_annotate_url(annotate_api_base: &str) -> Self {
        let executor = match action_enigo::EnigoBackend::new() {
            Ok(backend) => Arc::new(Mutex::new(ActionExecutor::new(Box::new(backend)))),
            Err(err) => {
                log::warn!("Failed to create enigo backend, computer tools will be unavailable: {}", err);
                Arc::new(Mutex::new(ActionExecutor::new(Box::new(FallbackBackend))))
            }
        };
        let vision_state = Arc::new(Mutex::new(VisionState::new()));
        let base_url = if annotate_api_base.is_empty() {
            DEFAULT_ANNOTATE_API_BASE
        } else {
            annotate_api_base
        };
        let annotate_client = AnnotateClient::with_base_url(base_url)
            .unwrap_or_else(|_| AnnotateClient::with_base_url(DEFAULT_ANNOTATE_API_BASE).expect("default annotate client should not fail"));
        Self {
            executor,
            vision_state,
            annotate_client,
        }
    }
}

/// Fallback backend for when enigo fails to initialize.
#[derive(Debug, Default)]
struct FallbackBackend;

impl actions::ActionBackend for FallbackBackend {
    fn click(&self) -> anyhow::Result<actions::ActionResult> {
        Err(anyhow::anyhow!("Action backend not available"))
    }

    fn double_click(&self) -> anyhow::Result<actions::ActionResult> {
        Err(anyhow::anyhow!("Action backend not available"))
    }

    fn right_click(&self) -> anyhow::Result<actions::ActionResult> {
        Err(anyhow::anyhow!("Action backend not available"))
    }

    fn move_to(&self, _x: i32, _y: i32) -> anyhow::Result<actions::ActionResult> {
        Err(anyhow::anyhow!("Action backend not available"))
    }

    fn scroll(&self, _lines: i32) -> anyhow::Result<actions::ActionResult> {
        Err(anyhow::anyhow!("Action backend not available"))
    }

    fn type_text(&self, _text: &str) -> anyhow::Result<actions::ActionResult> {
        Err(anyhow::anyhow!("Action backend not available"))
    }

    fn hotkey(&self, _keys: &[&str]) -> anyhow::Result<actions::ActionResult> {
        Err(anyhow::anyhow!("Action backend not available"))
    }

    fn get_position(&self) -> anyhow::Result<(i32, i32)> {
        Err(anyhow::anyhow!("Action backend not available"))
    }

    fn key_phase(
        &self,
        _name: &str,
        _phase: actions::KeyPhase,
    ) -> anyhow::Result<actions::ActionResult> {
        Err(anyhow::anyhow!("Action backend not available"))
    }

    fn mouse_phase(
        &self,
        _button: actions::MouseButton,
        _phase: actions::KeyPhase,
    ) -> anyhow::Result<actions::ActionResult> {
        Err(anyhow::anyhow!("Action backend not available"))
    }
}

/// Register all computer use tools with the given state.
///
/// # Arguments
/// * `reg` - The tool registry to register with.
/// * `state` - The shared computer state for all tools.
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
