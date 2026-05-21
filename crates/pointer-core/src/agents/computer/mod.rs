//! Computer agent: foundation (actions, screen, state) in this module; **tool** handlers in [`tools`].

pub mod action_enigo;
pub mod actions;
pub mod annotate;
pub mod coord;
mod mouse_move;
mod mouse_path;
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
pub mod reference_anchors;

pub use timing::{
    is_desktop_post_delay_tool, is_desktop_vision_log_tool, post_desktop_action_delay_ms_from_tool_args,
    COMPOSITE_ACTION_STEP_GAP_MS, POST_DESKTOP_ACTION_DELAY_MS, POST_DESKTOP_ACTION_WAIT_SEC_MAX,
    POST_DESKTOP_ACTION_WAIT_SEC_MIN,
};

use crate::agents::AgentRegistry;
use crate::platform_auth::SharedPlatformAuth;

use actions::ActionExecutor;
use annotate::AnnotateClient;
use coord::CoordinateSystem;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use screen_overlay::{build_before_action_inject, build_vision_overlay_pack};
use vision_state::VisionState;

/// Default annotation service URL.
const DEFAULT_ANNOTATE_API_BASE: &str = "http://127.0.0.1:8000";
/// Config key for the annotation service URL in Computer Agent's config.
const CONFIG_KEY_ANNOTATE_API_BASE: &str = "annotateApiBase";
/// Config key for default human-like mouse movement in Computer Agent's config.
const CONFIG_KEY_COMPUTER_HUMAN_LIKE: &str = "computerHumanLike";
/// Maximum number of concurrent sessions to retain before evicting the least recently used.
const MAX_SESSIONS: usize = 10;

/// Successful capture + annotation for one model turn (consumers: screen inject, UI preview).
#[derive(Debug, Clone)]
pub struct ScreenCaptureResult {
    /// OS capture JPEG **before** synthetic pointer/caret overlay (debug / inspection only).
    pub raw_unmarked_jpeg: Vec<u8>,
    /// Marked raw JPEG for this turn (pointer/caret drawn after annotate step).
    pub raw_marked_jpeg: Vec<u8>,
    /// Marked annotated PNG (indices from service + pointer/caret).
    pub annotated_marked_png: Vec<u8>,
    /// Zoom: top 100px of marked annotated (menu bar).
    pub zoom_menu_bar_png: Vec<u8>,
    /// Zoom: bottom 100px of marked annotated (task bar).
    pub zoom_task_bar_png: Vec<u8>,
    /// Zoom: 200×200 crop, 4× magnified (800×800) around pointer on marked annotated.
    pub zoom_pointer_png: Vec<u8>,
    /// Optional prose: **Pointer position** + **`[Zoom pointer after action]`** coordinate-anchor guidance (see `reference_anchors`).
    pub mouse_neighbor_reference_text: Option<String>,
    /// Logical monitor bounds for this capture.
    pub monitor: screen::MonitorInfo,
    /// **`[Screen before action]`** + **`[Zoom pointer before action]`** (`None` on first capture).
    pub inject_before_action: Option<screen_overlay::BeforeActionInject>,
}

/// Per-conversation state for computer use tools.
/// Each conversation gets its own instance, managed by [`ComputerState`].
/// Maximum recent desktop tool entries kept per session.
const MAX_RECENT_DESKTOP_TOOLS: usize = 10;

/// A single remembered desktop tool invocation for [Recent desktop tool calls] prompt block.
pub struct DesktopToolEntry {
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub failure_note: Option<String>,
}

/// Session lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Active,
    Ended,
    Cancelled,
}

pub struct ComputerSession {
    pub vision_state: Arc<Mutex<VisionState>>,
    pub last_annotated: Option<(Vec<u8>, screen::MonitorInfo)>,
    /// Prior turn’s **unmarked** capture JPEG (same input as annotate that turn).
    pub last_turn_raw_jpeg_unmarked: Option<Vec<u8>>,
    pub last_turn_monitor: Option<screen::MonitorInfo>,
    pub selected_monitor: Option<String>,
    pub created_at: u64,
    pub last_active_at: u64,
    pub status: SessionStatus,
    pub desktop_log: Vec<DesktopToolEntry>,
}

impl ComputerSession {
    fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            vision_state: Arc::new(Mutex::new(VisionState::new())),
            last_annotated: None,
            last_turn_raw_jpeg_unmarked: None,
            last_turn_monitor: None,
            selected_monitor: None,
            created_at: now,
            last_active_at: now,
            status: SessionStatus::Active,
            desktop_log: Vec::new(),
        }
    }
}

/// Shared state for computer use tools.
/// Holds the action executor, annotation client, and per-conversation sessions.
pub struct ComputerState {
    /// The action executor.
    pub executor: Arc<Mutex<ActionExecutor>>,
    /// The annotation service client.
    pub annotate_client: AnnotateClient,
    /// Default for `human_like` when omitted from tool args (Python `computer_human_like`).
    pub human_like_default: bool,
    sessions: Arc<RwLock<HashMap<String, Arc<Mutex<ComputerSession>>>>>,
}

impl std::fmt::Debug for ComputerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sessions_snapshot = self.sessions.read().ok().map(|g| g.len());
        f.debug_struct("ComputerState")
            .field("active_sessions", &sessions_snapshot)
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
    pub fn new(agents: &AgentRegistry, platform_auth: SharedPlatformAuth) -> Self {
        let def = agents.get("computer").map(|agent| agent.def());
        let annotate_api_base = def
            .as_ref()
            .and_then(|d| d.config.get(CONFIG_KEY_ANNOTATE_API_BASE).cloned())
            .unwrap_or_else(|| DEFAULT_ANNOTATE_API_BASE.to_string());
        let human_like_default = def
            .as_ref()
            .and_then(|d| d.config.get(CONFIG_KEY_COMPUTER_HUMAN_LIKE))
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);
        Self::with_annotate_url_and_human_like(&annotate_api_base, human_like_default, Some(platform_auth))
    }

    /// Create a new ComputerState with an explicit annotation service URL.
    ///
    /// # Arguments
    /// * `annotate_api_base` - Base URL for the annotation service. If empty, uses the default.
    pub fn with_annotate_url(annotate_api_base: &str) -> Self {
        Self::with_annotate_url_and_human_like(annotate_api_base, false, None)
    }

    /// Create state with explicit annotation URL and human-like default.
    pub fn with_annotate_url_and_human_like(
        annotate_api_base: &str,
        human_like_default: bool,
        platform_auth: Option<SharedPlatformAuth>,
    ) -> Self {
        let executor = match action_enigo::EnigoBackend::new() {
            Ok(backend) => Arc::new(Mutex::new(ActionExecutor::new(Box::new(backend)))),
            Err(err) => {
                log::warn!("Failed to create enigo backend, computer tools will be unavailable: {}", err);
                Arc::new(Mutex::new(ActionExecutor::new(Box::new(FallbackBackend))))
            }
        };
        let base_url = if annotate_api_base.is_empty() {
            DEFAULT_ANNOTATE_API_BASE
        } else {
            annotate_api_base
        };
        let annotate_client = AnnotateClient::with_base_url_and_auth(base_url, platform_auth)
            .unwrap_or_else(|_| {
                AnnotateClient::with_base_url_and_auth(DEFAULT_ANNOTATE_API_BASE, None)
                    .expect("default annotate client should not fail")
            });
        Self {
            executor,
            annotate_client,
            human_like_default,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Retrieve or create the session for a conversation, updating last_active_at and evicting if needed.
    /// Eviction priority: ended → cancelled → least recently active among active sessions.
    fn get_or_create_session(&self, conversation_id: &str) -> Arc<Mutex<ComputerSession>> {
        let mut sessions = self.sessions.write().unwrap_or_else(|e| {
            log::warn!("sessions map RwLock poisoned; recovering");
            e.into_inner()
        });

        if let Some(entry) = sessions.get(conversation_id) {
            let mut session = entry.lock().unwrap();
            session.last_active_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if session.status == SessionStatus::Active {
                // already active, just return
            }
            return Arc::clone(entry);
        }

        // Evict if at capacity: first ended, then cancelled, then LRU among active
        while sessions.len() >= MAX_SESSIONS {
            // Phase 1: remove any Ended session
            let ended_id = sessions
                .iter()
                .find(|(_, s)| s.lock().unwrap().status == SessionStatus::Ended)
                .map(|(id, _)| id.clone());
            if let Some(id) = ended_id {
                sessions.remove(&id);
                continue;
            }
            // Phase 2: remove any Cancelled session
            let cancelled_id = sessions
                .iter()
                .find(|(_, s)| s.lock().unwrap().status == SessionStatus::Cancelled)
                .map(|(id, _)| id.clone());
            if let Some(id) = cancelled_id {
                sessions.remove(&id);
                continue;
            }
            // Phase 3: remove least recently active among Active sessions
            let lru_id = sessions
                .iter()
                .filter(|(_, s)| s.lock().unwrap().status == SessionStatus::Active)
                .min_by_key(|(_, s)| s.lock().unwrap().last_active_at)
                .map(|(id, _)| id.clone());
            if let Some(id) = lru_id {
                sessions.remove(&id);
            } else {
                // No active sessions but map still full (shouldn't happen), just break
                break;
            }
        }

        let session = Arc::new(Mutex::new(ComputerSession::new()));
        sessions.insert(conversation_id.to_string(), Arc::clone(&session));
        session
    }

    /// Mark the session for `conversation_id` as ended (conversation finished normally).
    pub fn mark_ended(&self, conversation_id: &str) {
        let session = self.get_or_create_session(conversation_id);
        session.lock().unwrap().status = SessionStatus::Ended;
    }

    /// Mark the session for `conversation_id` as cancelled (conversation aborted).
    pub fn mark_cancelled(&self, conversation_id: &str) {
        let session = self.get_or_create_session(conversation_id);
        session.lock().unwrap().status = SessionStatus::Cancelled;
    }

    /// Set the selected monitor id for a conversation (Computer agent).
    ///
    /// `monitor_id = None` resets to auto mode (monitor under cursor).
    pub fn set_conversation_monitor(&self, conversation_id: &str, monitor_id: Option<String>) {
        let session = self.get_or_create_session(conversation_id);
        let mut s = session.lock().unwrap();
        s.selected_monitor = monitor_id;
    }

    fn selected_monitor_id_for_conversation(&self, conversation_id: &str) -> Option<String> {
        let session = self.get_or_create_session(conversation_id);
        let s = session.lock().unwrap();
        s.selected_monitor.clone()
    }

    /// Run annotation + vision refresh for an already-captured desktop JPEG (integration tests, tooling).
    ///
    /// On success, stores **unmarked** `screen_capture` for the next turn’s **`[Screen before action]`** inject.
    pub async fn apply_screen_capture(
        &self,
        conversation_id: &str,
        screen_capture: &[u8],
        monitor: screen::MonitorInfo,
        capture_px: (u32, u32),
        global_pointer: (i32, i32),
        global_caret: Option<(i32, i32)>,
    ) -> anyhow::Result<ScreenCaptureResult> {
        let t_total = Instant::now();

        let session = self.get_or_create_session(conversation_id);
        let inject_before_action = {
            let s = session.lock().unwrap();
            match (
                s.last_turn_raw_jpeg_unmarked.as_deref(),
                s.last_turn_monitor.as_ref(),
            ) {
                (Some(jpeg), Some(mon)) => match build_before_action_inject(jpeg, mon, global_pointer) {
                    Ok(pack) => Some(pack),
                    Err(e) => {
                        log::warn!(
                            "apply_screen_capture: build before-action inject from prior unmarked failed: {:#}",
                            e
                        );
                        None
                    }
                },
                _ => None,
            }
        };

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
        {
            let session_guard = session.lock().unwrap();
            let mut vs = session_guard.vision_state.lock().unwrap();
            vs.set_screen_bbox(monitor);
            vs.set_index_map_from_boxes(&boxes, &monitor, capture_px);
            vs.set_coordinate_system(CoordinateSystem::Qwen);
        }
        let vision_ms = t.elapsed().as_secs_f64() * 1000.0;

        let mouse_neighbor_reference_text =
            reference_anchors::format_mouse_neighbor_reference_bboxes(
                &boxes,
                &monitor,
                capture_px,
                global_pointer,
                CoordinateSystem::Qwen,
            );

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

        {
            let mut session = session.lock().unwrap();
            session.last_turn_raw_jpeg_unmarked = Some(screen_capture.to_vec());
            session.last_turn_monitor = Some(monitor);
            session.last_annotated = Some((pack.annotated_marked_png.clone(), monitor));
        }

        let total_ms = t_total.elapsed().as_secs_f64() * 1000.0;
        log::info!(
            "apply_screen_capture: annotate_http {:.1}ms, vision_state {:.1}ms, overlay_zoom {:.1}ms, total {:.1}ms ({} boxes)",
            annotate_ms,
            vision_ms,
            overlay_ms,
            total_ms,
            boxes.len()
        );

        Ok(ScreenCaptureResult {
            raw_unmarked_jpeg: screen_capture.to_vec(),
            raw_marked_jpeg: pack.raw_marked_jpeg,
            annotated_marked_png: pack.annotated_marked_png,
            zoom_menu_bar_png: pack.zoom_menu_bar_png,
            zoom_task_bar_png: pack.zoom_task_bar_png,
            zoom_pointer_png: pack.zoom_pointer_png,
            mouse_neighbor_reference_text,
            monitor,
            inject_before_action,
        })
    }

    /// Capture the display under the cursor, call the annotation service, and refresh state for this session.
    pub async fn capture_and_annotate(&self, conversation_id: &str) -> anyhow::Result<ScreenCaptureResult> {
        let t_total = Instant::now();

        let t = Instant::now();
        let monitor_id = self.selected_monitor_id_for_conversation(conversation_id);
        let shot = tokio::task::spawn_blocking(move || match monitor_id.as_deref() {
            Some(id) => screen::screenshot_monitor_by_id(id),
            None => screen::screenshot_current_monitor(),
        })
        .await
        .map_err(|e| anyhow::anyhow!("screenshot task join: {e}"))??;
        let screen_ms = t.elapsed().as_secs_f64() * 1000.0;

        let out = self
            .apply_screen_capture(
                conversation_id,
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

    /// Latest annotated screenshot (PNG bytes already shown to the model) for the given conversation, if any.
    pub fn cached_annotated_for_conversation(&self, conversation_id: &str) -> Option<(Vec<u8>, screen::MonitorInfo)> {
        let session = self.get_or_create_session(conversation_id);
        let s = session.lock().unwrap();
        s.last_annotated.clone()
    }

    /// Get a reference to the vision state for a specific conversation.
    pub fn vision_state_for_conversation(&self, conversation_id: &str) -> Arc<Mutex<VisionState>> {
        let session = self.get_or_create_session(conversation_id);
        let guard = session.lock().unwrap();
        Arc::clone(&guard.vision_state)
    }

    /// Log a desktop tool invocation for the conversation's [Recent desktop tool calls] snippet.
    pub fn record_desktop_tool_if_applicable(
        &self,
        conversation_id: &str,
        tool_name: &str,
        args: &serde_json::Value,
        failure_note: Option<&str>,
    ) {
        if !is_desktop_vision_log_tool(tool_name) {
            return;
        }
        let session = self.get_or_create_session(conversation_id);
        let mut s = session.lock().unwrap();
        let entry = DesktopToolEntry {
            tool_name: tool_name.to_string(),
            arguments: args.clone(),
            failure_note: failure_note.map(str::to_string),
        };
        s.desktop_log.push(entry);
        if s.desktop_log.len() > MAX_RECENT_DESKTOP_TOOLS {
            s.desktop_log.remove(0);
        }
    }

    /// Build the [Recent desktop tool calls] prompt block for a conversation.
    pub fn recent_actions_prompt_block(&self, conversation_id: &str) -> Option<String> {
        let session = self.get_or_create_session(conversation_id);
        let s = session.lock().unwrap();
        let log = &s.desktop_log;
        if log.is_empty() {
            return None;
        }
        let mut lines: Vec<String> = Vec::with_capacity(log.len() + 2);
        lines.push("[Recent desktop tool calls]".to_string());
        for (i, entry) in log.iter().rev().enumerate() {
            let mut line = format!(
                "  {}: {} {}",
                log.len() - i,
                entry.tool_name,
                entry.arguments
            );
            if let Some(ref fail) = entry.failure_note {
                line.push_str(&format!(" (FAILED: {fail})"));
            }
            lines.push(line);
        }
        Some(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    fn make_state() -> ComputerState {
        ComputerState::with_annotate_url("http://127.0.0.1:9999")
    }

    #[test]
    fn session_create_and_reuse() {
        let state = make_state();
        let s1 = state.vision_state_for_conversation("a");
        let s2 = state.vision_state_for_conversation("a");
        assert!(Arc::ptr_eq(&s1, &s2), "same conversation_id returns same session");
    }

    #[test]
    fn distinct_sessions_are_isolated() {
        let state = make_state();
        let sa = state.vision_state_for_conversation("a");
        let sb = state.vision_state_for_conversation("b");
        assert!(!Arc::ptr_eq(&sa, &sb), "different conversation_id must yield different sessions");
    }

    #[test]
    fn mark_ended_and_cancelled_status() {
        let state = make_state();
        state.mark_ended("x");
        assert_eq!(state.get_or_create_session("x").lock().unwrap().status, SessionStatus::Ended);

        state.mark_cancelled("y");
        assert_eq!(state.get_or_create_session("y").lock().unwrap().status, SessionStatus::Cancelled);
    }

    #[test]
    fn eviction_prioritises_ended_over_active() {
        let state = make_state();
        // Fill capacity with active sessions
        for i in 0..MAX_SESSIONS {
            state.get_or_create_session(&format!("conv-{}", i));
        }
        // Mark a few as ended
        state.mark_ended("conv-0");
        state.mark_ended("conv-2");
        // Add another session – should evict ended sessions first
        state.get_or_create_session("overflow");
        // Ended sessions may have been evicted; check that at most one ended survived
        let s0 = state.get_or_create_session("conv-0");
        let s2 = state.get_or_create_session("conv-2");
        let ended_before = [&s0, &s2]
            .iter()
            .filter(|s| s.lock().unwrap().status == SessionStatus::Ended)
            .count();
        assert!(ended_before <= 1, "at most one ended session should survive after evicting ended first");
    }

    #[test]
    fn eviction_fallback_to_lru_for_active() {
        let state = make_state();
        for i in 0..MAX_SESSIONS {
            state.get_or_create_session(&format!("conv-{}", i));
            thread::sleep(Duration::from_millis(5)); // ensure distinct last_active_at
        }
        // conv-0 is oldest; overflow should evict it
        state.get_or_create_session("overflow");
        // conv-0 should be fresh now (recreated)
        let s0 = state.get_or_create_session("conv-0");
        assert_eq!(s0.lock().unwrap().status, SessionStatus::Active);
    }

    #[test]
    fn monitor_per_conversation_isolation() {
        let state = make_state();
        state.set_conversation_monitor("a", Some("m1".into()));
        state.set_conversation_monitor("b", None);
        assert_eq!(state.get_or_create_session("a").lock().unwrap().selected_monitor.as_deref(), Some("m1"));
        assert!(state.get_or_create_session("b").lock().unwrap().selected_monitor.is_none());
    }
}

/// Placeholder backend when enigo fails to initialize.
struct FallbackBackend;

impl actions::ActionBackend for FallbackBackend {
    fn click(&self) -> anyhow::Result<actions::ActionResult> {
        anyhow::bail!("enigo backend not available; computer actions are disabled")
    }
    fn double_click(&self) -> anyhow::Result<actions::ActionResult> {
        anyhow::bail!("enigo backend not available; computer actions are disabled")
    }
    fn right_click(&self) -> anyhow::Result<actions::ActionResult> {
        anyhow::bail!("enigo backend not available; computer actions are disabled")
    }
    fn move_to_with_profile(
        &self,
        _x: i32,
        _y: i32,
        _profile: mouse_move::MouseMoveProfile,
    ) -> anyhow::Result<actions::ActionResult> {
        anyhow::bail!("enigo backend not available; computer actions are disabled")
    }
    fn scroll(&self, _lines: i32) -> anyhow::Result<actions::ActionResult> {
        anyhow::bail!("enigo backend not available; computer actions are disabled")
    }
    fn type_text(&self, _text: &str) -> anyhow::Result<actions::ActionResult> {
        anyhow::bail!("enigo backend not available; computer actions are disabled")
    }
    fn hotkey(&self, _keys: &[&str]) -> anyhow::Result<actions::ActionResult> {
        anyhow::bail!("enigo backend not available; computer actions are disabled")
    }
    fn get_position(&self) -> anyhow::Result<(i32, i32)> {
        anyhow::bail!("enigo backend not available; computer actions are disabled")
    }
    fn key_phase(&self, _name: &str, _phase: actions::KeyPhase) -> anyhow::Result<actions::ActionResult> {
        anyhow::bail!("enigo backend not available; computer actions are disabled")
    }
    fn mouse_phase(&self, _button: actions::MouseButton, _phase: actions::KeyPhase) -> anyhow::Result<actions::ActionResult> {
        anyhow::bail!("enigo backend not available; computer actions are disabled")
    }
}
