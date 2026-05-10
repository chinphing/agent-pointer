//! Computer agent: foundation (actions, screen, state) in this module; **tool** handlers in [`tools`].

pub mod action_enigo;
pub mod actions;
pub mod annotate;
pub mod coord;
/// Computer-specific [`crate::extensions`] hooks (e.g. screen inject).
pub mod extension_hooks;
pub mod screen;
/// ToolRegistry handlers, JSON schemas, and prompts for computer use.
pub mod tools;
pub mod verify;
pub mod vision_state;

use crate::agents::AgentRegistry;
use crate::models::ComputerAnnotatedPreview;
use actions::ActionExecutor;
use annotate::AnnotateClient;
use coord::CoordinateSystem;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use vision_state::VisionState;

/// Default annotation service URL.
const DEFAULT_ANNOTATE_API_BASE: &str = "http://127.0.0.1:8000";
/// Config key for the annotation service URL in Computer Agent's config.
const CONFIG_KEY_ANNOTATE_API_BASE: &str = "annotateApiBase";

/// Shared state for computer use tools.
///
/// This struct is created once in AppState and shared across all computer tool handlers.
/// It holds the action executor, vision state, and annotation client for the current session.
pub struct ComputerState {
    /// The action executor.
    pub executor: Arc<Mutex<ActionExecutor>>,
    /// The current vision state.
    pub vision_state: Arc<Mutex<VisionState>>,
    /// The annotation service client.
    pub annotate_client: AnnotateClient,
    /// Last successful annotated PNG + monitor bounds.
    /// Written by [`Self::capture_and_annotate`] (including `_10_computer_screen_inject`); UI preview reads this for parity with model vision input.
    last_annotated: Arc<Mutex<Option<(Vec<u8>, screen::MonitorInfo)>>>,
}

impl std::fmt::Debug for ComputerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let snap = self
            .last_annotated
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|(b, m)| (b.len(), *m)));
        f.debug_struct("ComputerState")
            .field("last_annotated_png_len_and_monitor", &snap)
            .finish_non_exhaustive()
    }
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
            last_annotated: Arc::new(Mutex::new(None)),
        }
    }

    /// Capture the display under the cursor, call the annotation service, and refresh [`VisionState`].
    ///
    /// Returns PNG bytes of the **annotated** image (for model input) and the [`screen::MonitorInfo`]
    /// for that capture. Raw capture is JPEG from [`screen::screenshot_current_monitor`] (fast encode);
    /// the annotate client still uploads a prepared PNG to the service. Capture uses `xcap`.
    pub async fn capture_and_annotate(
        &self,
    ) -> anyhow::Result<(Vec<u8>, screen::MonitorInfo)> {
        let t_total = Instant::now();

        let t = Instant::now();
        let (screen_capture, monitor, capture_px) = screen::screenshot_current_monitor()?;
        let screen_ms = t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        let (annotated, boxes) = self.annotate_client.annotate_image(&screen_capture).await?;
        let annotate_ms = t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        let mut vision = self.vision_state.lock().unwrap();
        vision.set_screen_bbox(monitor);
        vision.set_index_map_from_boxes(&boxes, &monitor, capture_px);
        vision.set_coordinate_system(CoordinateSystem::Qwen);
        drop(vision);
        let vision_ms = t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        if let Ok(mut g) = self.last_annotated.lock() {
            *g = Some((annotated.clone(), monitor));
        }
        let cache_ms = t.elapsed().as_secs_f64() * 1000.0;

        let total_ms = t_total.elapsed().as_secs_f64() * 1000.0;
        log::info!(
            "capture_and_annotate: screenshot {:.1}ms, annotate_http {:.1}ms, vision_state {:.1}ms, last_frame_cache {:.1}ms, total {:.1}ms ({} boxes)",
            screen_ms,
            annotate_ms,
            vision_ms,
            cache_ms,
            total_ms,
            boxes.len()
        );

        Ok((annotated, monitor))
    }

    /// Latest annotated screenshot (PNG bytes already shown to the model), if any.
    pub fn cached_annotated_preview(&self) -> Option<ComputerAnnotatedPreview> {
        let g = self.last_annotated.lock().ok()?;
        let (png, monitor) = g.as_ref()?;
        Some(ComputerAnnotatedPreview {
            image_base64: screen::encode_image_to_base64(png),
            caption: format!(
                "Annotated desktop · global bounds (px): left={} top={} width={} height={}",
                monitor.left, monitor.top, monitor.width, monitor.height
            ),
        })
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
