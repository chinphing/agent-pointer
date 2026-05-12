//! Computer agent: foundation (actions, screen, state) in this module; **tool** handlers in [`tools`].

pub mod action_enigo;
pub mod actions;
pub mod annotate;
pub mod coord;
/// Computer-specific [`crate::extensions`] hooks (e.g. screen inject).
pub mod extension_hooks;
pub mod screen;
pub mod screen_overlay;
/// ToolRegistry handlers, JSON schemas, and prompts for computer use.
pub mod tools;
pub mod verify;
pub mod vision_state;
pub mod timing;
pub mod capture_debug;

pub use timing::{
    is_desktop_post_delay_tool, is_desktop_vision_log_tool, COMPOSITE_ACTION_STEP_GAP_MS,
    POST_DESKTOP_ACTION_DELAY_MS,
};

use crate::agents::AgentRegistry;
use crate::models::ComputerAnnotatedPreview;
use actions::ActionExecutor;
use annotate::AnnotateClient;
use coord::CoordinateSystem;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use screen_overlay::build_vision_overlay_pack;
use vision_state::VisionState;

/// Default annotation service URL.
const DEFAULT_ANNOTATE_API_BASE: &str = "http://127.0.0.1:8000";
/// Config key for the annotation service URL in Computer Agent's config.
const CONFIG_KEY_ANNOTATE_API_BASE: &str = "annotateApiBase";

/// Successful capture + annotation for one model turn (consumers: screen inject, UI preview).
#[derive(Debug, Clone)]
pub struct ScreenCaptureResult {
    /// Marked raw JPEG for this turn (pointer/caret drawn after annotate step).
    pub raw_marked_jpeg: Vec<u8>,
    /// Marked annotated PNG (indices from service + pointer/caret).
    pub annotated_marked_png: Vec<u8>,
    /// Zoom: top 100px of marked annotated (menu bar).
    pub zoom_menu_bar_png: Vec<u8>,
    /// Zoom: bottom 100px of marked annotated (task bar).
    pub zoom_task_bar_png: Vec<u8>,
    /// Zoom: ≤300×300 around pointer on marked annotated.
    pub zoom_pointer_png: Vec<u8>,
    /// Logical monitor bounds for this capture.
    pub monitor: screen::MonitorInfo,
    /// Previous turn’s **marked** raw JPEG (`None` on first capture in a session).
    pub inject_previous_raw_jpeg: Option<Vec<u8>>,
}

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
    /// Raw JPEG from the last successful capture; offered as “previous turn” on the **next** `[CUR_SCREEN]` inject.
    last_turn_raw_jpeg: Arc<Mutex<Option<Vec<u8>>>>,
    /// Selected monitor id per conversation; `None` means auto (monitor under cursor).
    selected_monitor_by_conversation: Arc<Mutex<HashMap<String, Option<String>>>>,
}

impl std::fmt::Debug for ComputerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let snap = self
            .last_annotated
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|(b, m)| (b.len(), *m)));
        let prev_raw = self
            .last_turn_raw_jpeg
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|b| b.len()));
        f.debug_struct("ComputerState")
            .field("last_annotated_png_len_and_monitor", &snap)
            .field("last_turn_raw_jpeg_bytes", &prev_raw)
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
            last_turn_raw_jpeg: Arc::new(Mutex::new(None)),
            selected_monitor_by_conversation: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Set the selected monitor id for a conversation (Computer agent).
    ///
    /// `monitor_id = None` resets to auto mode (monitor under cursor).
    pub fn set_conversation_monitor(&self, conversation_id: &str, monitor_id: Option<String>) {
        let mut guard = self.selected_monitor_by_conversation.lock().unwrap_or_else(|e| {
            log::warn!("selected monitor map mutex poisoned; recovering");
            e.into_inner()
        });
        guard.insert(conversation_id.to_string(), monitor_id);
    }

    fn selected_monitor_id_for_conversation(&self, conversation_id: &str) -> Option<String> {
        let guard = self.selected_monitor_by_conversation.lock().ok()?;
        guard.get(conversation_id).cloned().flatten()
    }

    /// Run annotation + vision refresh for an already-captured desktop JPEG (integration tests, tooling).
    ///
    /// On success, stores `screen_capture` for use as the **previous** raw on the next inject.
    pub async fn apply_screen_capture(
        &self,
        screen_capture: &[u8],
        monitor: screen::MonitorInfo,
        capture_px: (u32, u32),
        global_pointer: (i32, i32),
        global_caret: Option<(i32, i32)>,
    ) -> anyhow::Result<ScreenCaptureResult> {
        let t_total = Instant::now();

        let inject_previous_raw_jpeg = self.last_turn_raw_jpeg.lock().unwrap().clone();

        let t = Instant::now();
        let ann = self
            .annotate_client
            .annotate_image(screen_capture)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        let annotate_ms = t.elapsed().as_secs_f64() * 1000.0;
        let annotated_png = ann.image;
        let boxes = ann.boxes;

        let t = Instant::now();
        let mut vision = self.vision_state.lock().unwrap();
        vision.set_screen_bbox(monitor);
        vision.set_index_map_from_boxes(&boxes, &monitor, capture_px);
        vision.set_coordinate_system(CoordinateSystem::Qwen);
        drop(vision);
        let vision_ms = t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        let pack = build_vision_overlay_pack(
            screen_capture,
            &annotated_png,
            &monitor,
            global_pointer,
            global_caret,
        )
        .map_err(|e| anyhow::anyhow!(e))?;
        let overlay_ms = t.elapsed().as_secs_f64() * 1000.0;

        *self.last_turn_raw_jpeg.lock().unwrap() = Some(pack.raw_marked_jpeg.clone());

        let t = Instant::now();
        if let Ok(mut g) = self.last_annotated.lock() {
            *g = Some((pack.annotated_marked_png.clone(), monitor));
        }
        let cache_ms = t.elapsed().as_secs_f64() * 1000.0;

        let total_ms = t_total.elapsed().as_secs_f64() * 1000.0;
        log::info!(
            "apply_screen_capture: annotate_http {:.1}ms, vision_state {:.1}ms, overlay_zoom {:.1}ms, last_frame_cache {:.1}ms, total {:.1}ms ({} boxes)",
            annotate_ms,
            vision_ms,
            overlay_ms,
            cache_ms,
            total_ms,
            boxes.len()
        );

        Ok(ScreenCaptureResult {
            raw_marked_jpeg: pack.raw_marked_jpeg,
            annotated_marked_png: pack.annotated_marked_png,
            zoom_menu_bar_png: pack.zoom_menu_bar_png,
            zoom_task_bar_png: pack.zoom_task_bar_png,
            zoom_pointer_png: pack.zoom_pointer_png,
            monitor,
            inject_previous_raw_jpeg,
        })
    }

    /// Capture the display under the cursor, call the annotation service, and refresh [`VisionState`].
    ///
    /// Raw capture is JPEG from [`screen::screenshot_current_monitor`] (fast encode); the annotate
    /// client uploads a prepared PNG to the service. Capture uses `xcap`.
    pub async fn capture_and_annotate(&self, conversation_id: &str) -> anyhow::Result<ScreenCaptureResult> {
        let t_total = Instant::now();

        let t = Instant::now();
        let shot = match self.selected_monitor_id_for_conversation(conversation_id) {
            Some(id) => screen::screenshot_monitor_by_id(&id)?,
            None => screen::screenshot_current_monitor()?,
        };
        let screen_ms = t.elapsed().as_secs_f64() * 1000.0;

        let out = self
            .apply_screen_capture(
                &shot.jpeg,
                shot.monitor,
                shot.capture_px,
                shot.global_pointer,
                shot.global_caret,
            )
            .await?;

        log::info!(
            "capture_and_annotate: screenshot {:.1}ms, total_with_pipeline {:.1}ms",
            screen_ms,
            t_total.elapsed().as_secs_f64() * 1000.0
        );

        Ok(out)
    }

    /// Latest annotated screenshot (PNG bytes already shown to the model), if any.
    pub fn cached_annotated_preview(&self) -> Option<ComputerAnnotatedPreview> {
        let g = self.last_annotated.lock().ok()?;
        let (png, _monitor) = g.as_ref()?;
        Some(ComputerAnnotatedPreview {
            image_base64: screen::encode_image_to_base64(png),
            caption: "Annotated desktop".into(),
        })
    }

    /// Record a desktop tool call for repetition hints under `[CUR_SCREEN]` (`goal` / `action` when present).
    /// Pass `failed_note` when the invocation failed (tool returned failure or threw); it is shown as `FAILED: …`.
    pub fn record_desktop_tool_if_applicable(
        &self,
        tool_id: &str,
        args: &serde_json::Value,
        failed_note: Option<&str>,
    ) {
        if !timing::is_desktop_vision_log_tool(tool_id) {
            return;
        }
        let mut guard = self.vision_state.lock().unwrap_or_else(|e| {
            log::warn!("vision_state mutex poisoned; recovering for desktop tool history");
            e.into_inner()
        });
        guard.record_desktop_tool_invocation(tool_id, args, failed_note);
    }

    /// Text block appended under `[CUR_SCREEN]` with up to the last five desktop tool rows.
    pub fn recent_actions_prompt_block(&self) -> Option<String> {
        let guard = self.vision_state.lock().ok()?;
        guard.recent_actions_prompt_block()
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
