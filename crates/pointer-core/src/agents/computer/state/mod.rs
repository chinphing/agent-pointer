//! Per-conversation session, screen capture pipeline, and tier-aware desktop logging.

use crate::agents::computer::control::{ComputerControl, LocalControl};
use crate::agents::computer::input::actions::{self, ActionExecutor};
#[cfg(feature = "computer")]
use crate::agents::computer::input::enigo::EnigoBackend;
use crate::agents::computer::input::mouse_move::MouseMoveProfile;
use crate::agents::computer::input::timing::is_desktop_vision_log_tool;
use crate::agents::computer::pipeline::types::{AdvancedPipelineSession, VerifyConclusion};
use crate::agents::computer::pipeline::{verify_outcome_for_tier, verify_outcome_struct};
use crate::agents::computer::runtime::{
    format_history_block, normalize_tool_record, ComputerModelConfig, ComputerRuntime,
    PipelineLlmPhase,
};
use crate::agents::computer::vision::coord::CoordinateSystem;
use crate::agents::computer::vision::screen;
use crate::agents::computer::vision::screen_overlay::build_vision_overlay_pack;
use crate::agents::computer::vision::vision_state::VisionState;
use crate::agents::AgentRegistry;
use crate::models::ToolCall;
use crate::platform_auth::SharedPlatformAuth;
use crate::platform_config::SharedPlatformConfig;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Config key for the annotation service URL in Computer Agent's config.
/// Config key for default human-like mouse movement in Computer Agent's config.
/// Maximum number of concurrent sessions to retain before evicting the least recently used.
const MAX_SESSIONS: usize = 10;

/// Successful capture + annotation for one model turn (consumers: screen inject, UI preview).
#[derive(Debug, Clone)]
pub struct ScreenCaptureResult {
    /// OS capture JPEG **before** synthetic pointer/caret overlay (debug / inspection only).
    pub raw_unmarked_jpeg: Vec<u8>,
    /// The one screenshot the model sees: raw capture with the pointer and
    /// caret drawn on top. There used to be a second, annotated copy; it became
    /// identical once the annotation service was removed.
    pub screenshot_jpeg: Vec<u8>,
    /// Logical monitor bounds for this capture.
    pub monitor: screen::MonitorInfo,
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
    pub last_capture: Option<(Vec<u8>, screen::MonitorInfo)>,
    /// Current turn capture size in pixels.
    pub current_turn_capture_px: Option<(u32, u32)>,
    pub selected_monitor: Option<String>,
    /// Set when monitor is configured via API or sub-agent pick gate (incl. single-monitor auto).
    pub monitor_selection_done: bool,
    pub created_at: u64,
    pub last_active_at: u64,
    pub status: SessionStatus,
    pub desktop_log: Vec<DesktopToolEntry>,
    pub runtime: ComputerRuntime,
    pub pipeline: AdvancedPipelineSession,
}

impl ComputerSession {
    fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            vision_state: Arc::new(Mutex::new(VisionState::new())),
            last_capture: None,
            current_turn_capture_px: None,
            selected_monitor: None,
            monitor_selection_done: false,
            created_at: now,
            last_active_at: now,
            status: SessionStatus::Active,
            desktop_log: Vec::new(),
            runtime: ComputerRuntime::new(),
            pipeline: AdvancedPipelineSession::default(),
        }
    }
}

/// Shared state for computer use tools.
/// Holds the action executor, annotation client, and per-conversation sessions.
pub struct ComputerState {
    /// The action executor.
    pub executor: Arc<Mutex<ActionExecutor>>,
    /// L1 abstraction over the same executor. Tools that map onto the OSWorld
    /// 13 actions go through this, so a remote backend can be swapped in by
    /// changing one constructor instead of every tool.
    pub control: Arc<dyn ComputerControl>,
    /// The annotation service client.
    /// Default for `human_like` when omitted from tool args (Python `computer_human_like`).
    pub human_like_default: bool,
    /// Base tier options from agent manifest (overridden by app settings when loaded).
    pub pipeline_config: ComputerModelConfig,
    platform_config: Option<SharedPlatformConfig>,
    sessions: Arc<RwLock<HashMap<String, Arc<Mutex<ComputerSession>>>>>,
}

impl ComputerState {
    /// Merge agent manifest tier config with persisted user settings.
    pub fn effective_pipeline_config(&self) -> ComputerModelConfig {
        let mut cfg = self.pipeline_config.clone();
        if let Some(pc) = &self.platform_config {
            let platform = pc.read();
            let user = crate::storage::load_user_settings().unwrap_or_default();
            let settings = crate::models::merge_user_platform(&user, &platform);
            cfg.apply_platform_pipeline_llm(&settings.computer_pipeline_llm);
        }
        cfg
    }
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
    pub fn new(
        agents: &AgentRegistry,
        _platform_auth: SharedPlatformAuth,
        platform_config: SharedPlatformConfig,
    ) -> Self {
        // The user-facing switch lives in settings (`computerHumanLike`,
        // default true). AGENT.md does not carry it, so reading the agent
        // config here always yielded `false` and silently disabled human-like
        // mouse movement no matter what the UI showed.
        let human_like_default =
            crate::platform_config::effective_settings_global().computer_human_like;
        let pipeline_config = ComputerModelConfig::from_agent_registry(agents);
        Self::with_config(
            human_like_default,
            pipeline_config,
            Some(platform_config),
        )
    }

    /// Test convenience constructor with default pipeline config.
    pub fn default_for_test() -> Self {
        Self::with_config(false, ComputerModelConfig::default(), None)
    }

    pub fn with_config(
        human_like_default: bool,
        pipeline_config: ComputerModelConfig,
        platform_config: Option<SharedPlatformConfig>,
    ) -> Self {
        #[cfg(feature = "computer")]
        let executor = match EnigoBackend::new() {
            Ok(backend) => Arc::new(Mutex::new(ActionExecutor::new(Box::new(backend)))),
            Err(err) => {
                log::warn!(
                    "Failed to create enigo backend, computer tools will be unavailable: {}",
                    err
                );
                Arc::new(Mutex::new(ActionExecutor::new(Box::new(FallbackBackend))))
            }
        };
        #[cfg(not(feature = "computer"))]
        let executor = {
            log::debug!("computer feature disabled: desktop input backend unavailable");
            Arc::new(Mutex::new(ActionExecutor::new(Box::new(FallbackBackend))))
        };
        let control: Arc<dyn ComputerControl> =
            Arc::new(LocalControl::new(executor.clone(), human_like_default));
        Self {
            executor,
            control,
            human_like_default,
            pipeline_config,
            platform_config,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Returns true when the tier runtime has flagged that the task is exhausted
    /// (repetition_count reached GIVE_UP_THRESHOLD) and the outer loop should exit.
    pub fn should_give_up(&self, conversation_id: &str) -> bool {
        let session = self.get_or_create_session(conversation_id);
        let g = session.lock().unwrap().runtime.should_give_up;
        g
    }

    /// Clears give-up and stuck-loop state when the user sends new guidance.
    pub fn reset_for_new_user_guidance(&self, conversation_id: &str) {
        let session = self.get_or_create_session(conversation_id);
        session
            .lock()
            .unwrap()
            .runtime
            .reset_for_new_user_guidance();
    }

    /// Per-phase model for host verify pipeline (Verify phase).
    pub fn apply_pipeline_phase_settings(
        &self,
        conversation_id: &str,
        phase: PipelineLlmPhase,
        settings: &crate::models::ModelSettings,
    ) -> crate::models::ModelSettings {
        let cfg = self.effective_pipeline_config();
        let model = cfg.pipeline_llm.model_for_phase(phase).to_string();
        let provider = cfg.pipeline_llm.provider_for_phase(phase).to_string();
        let mut s = settings.clone();
        if !provider.trim().is_empty() {
            s.active_provider_id = provider.trim().to_string();
        }
        s.model = model;
        // Decision keeps the round thinking that the shared session-model path
        // already applied from `agentModeLlm[agentId][mode]`; Position / Verify /
        // Locate override it with their own budget (0 = thinking off).
        if !matches!(phase, PipelineLlmPhase::Decision) {
            self.apply_pipeline_thinking(phase, &mut s);
        }
        if crate::logging::internal_runtime_log_enabled() {
            log::debug!(
                "computer_pipeline_llm: conversation_id={conversation_id} phase={} provider={} model={} \
                 enable_thinking={:?} thinking_budget={:?}",
                phase.label(),
                s.active_provider_id,
                s.model,
                s.round_enable_thinking,
                s.round_thinking_budget
            );
        }
        s
    }

    /// Apply one phase's thinking intensity onto this round's settings.
    ///
    /// `off` locks thinking off; an absent value leaves the round unlocked so
    /// provider / model defaults apply. Verify and Locate default to `off`.
    pub fn apply_pipeline_thinking(
        &self,
        phase: PipelineLlmPhase,
        s: &mut crate::models::ModelSettings,
    ) {
        let cfg = self.effective_pipeline_config();
        crate::thinking_strategy::apply_thinking_intensity_to_round(
            s,
            cfg.pipeline_llm.thinking_intensity_for_phase(phase).as_deref(),
        );
    }

    /// Whether host post-execute verify is enabled (all tiers when true).
    pub fn verify_host_enabled(&self) -> bool {
        self.effective_pipeline_config().verify_host_enabled
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
        // A cancelled run may have left MOUSE_DOWN / KEY_DOWN unpaired; release
        // them now or every later click becomes a modified click.
        if let Ok(exec) = self.executor.lock() {
            let _ = exec.release_stranded_input();
        }
    }

    /// Set the selected monitor id for a conversation (Computer agent).
    ///
    /// `monitor_id = None` resets to auto mode (monitor under cursor).
    pub fn set_conversation_monitor(&self, conversation_id: &str, monitor_id: Option<String>) {
        let session = self.get_or_create_session(conversation_id);
        let mut s = session.lock().unwrap();
        let prev = s.selected_monitor.clone();
        s.selected_monitor = monitor_id.clone();
        s.monitor_selection_done = true;
        if prev.as_deref() != monitor_id.as_deref() {
            log::info!(
                "computer monitor: conversation_id={conversation_id} {} -> {}",
                prev.as_deref().unwrap_or("(none)"),
                monitor_id.as_deref().unwrap_or("(auto/cursor)")
            );
        }
    }

    pub fn is_monitor_selection_done(&self, conversation_id: &str) -> bool {
        let session = self.get_or_create_session(conversation_id);
        let done = session.lock().unwrap().monitor_selection_done;
        done
    }

    /// Current capture monitor id for a conversation (`None` = follow cursor).
    pub fn conversation_monitor_id(&self, conversation_id: &str) -> Option<String> {
        self.selected_monitor_id_for_conversation(conversation_id)
    }

    fn selected_monitor_id_for_conversation(&self, conversation_id: &str) -> Option<String> {
        let session = self.get_or_create_session(conversation_id);
        let s = session.lock().unwrap();
        s.selected_monitor.clone()
    }

    /// Run annotation + vision refresh for an already-captured desktop JPEG (integration tests, tooling).
    ///
    /// Stores this capture’s marked JPEG. The next turn reuses those bytes as the before frame.
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

        let t = Instant::now();
        {
            let session_guard = session.lock().unwrap();
            let mut vs = session_guard.vision_state.lock().unwrap();
            vs.set_screen_bbox(monitor);
            vs.set_coordinate_system(CoordinateSystem::Pixel);
        }
        let vision_ms = t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        let pack =
            build_vision_overlay_pack(screen_capture, &monitor, global_pointer, global_caret)
                .map_err(|e| anyhow::anyhow!(e))?;
        let overlay_ms = t.elapsed().as_secs_f64() * 1000.0;

        {
            let mut session = session.lock().unwrap();
            session.current_turn_capture_px = Some(capture_px);
            session.last_capture = Some((pack.screenshot_jpeg.clone(), monitor));
        }

        let total_ms = t_total.elapsed().as_secs_f64() * 1000.0;
        log::info!(
            "apply_screen_capture: input={} out={} | vision_state {:.1}ms, overlay {:.1}ms, total {:.1}ms ({}x{} px)",
            screen::format_data_size_bytes(screen_capture.len()),
            screen::format_data_size_bytes(pack.screenshot_jpeg.len()),
            vision_ms,
            overlay_ms,
            total_ms,
            capture_px.0,
            capture_px.1
        );

        Ok(ScreenCaptureResult {
            raw_unmarked_jpeg: screen_capture.to_vec(),
            screenshot_jpeg: pack.screenshot_jpeg,
            monitor,
        })
    }

    /// Capture the display under the cursor, call the annotation service, and refresh state for this session.
    ///
    /// Returns the capture result and an optional refreshed monitor id when a stale selection was recovered.
    pub async fn capture_screen(
        &self,
        conversation_id: &str,
    ) -> anyhow::Result<(ScreenCaptureResult, Option<String>)> {
        let t_total = Instant::now();

        let t = Instant::now();
        let monitor_id = self.selected_monitor_id_for_conversation(conversation_id);
        log::info!(
            "capture_screen: using monitor_id={} conversation_id={conversation_id}",
            monitor_id.as_deref().unwrap_or("(cursor/auto)")
        );
        let plan = tokio::task::spawn_blocking(move || {
            screen::screenshot_for_selection(monitor_id.as_deref())
        })
        .await
        .map_err(|e| anyhow::anyhow!("screenshot task join: {e}"))??;
        let shot = plan.packet;
        if let Some(ref new_id) = plan.refreshed_monitor_id {
            self.set_conversation_monitor(conversation_id, Some(new_id.clone()));
            log::info!(
                "capture_screen: monitor id refreshed conversation_id={conversation_id} new_id={new_id} kind={:?}",
                plan.resolve_kind
            );
        }
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
            "capture_screen: jpeg={} ({}x{} px) screenshot {:.1}ms, total_with_pipeline {:.1}ms",
            screen::format_data_size_bytes(shot.jpeg.len()),
            shot.capture_px.0,
            shot.capture_px.1,
            screen_ms,
            t_total.elapsed().as_secs_f64() * 1000.0
        );

        Ok((out, plan.refreshed_monitor_id))
    }

    /// Latest screenshot (PNG bytes already shown to the model) for the given conversation, if any.
    pub fn cached_capture_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Option<(Vec<u8>, screen::MonitorInfo)> {
        let session = self.get_or_create_session(conversation_id);
        let s = session.lock().unwrap();
        s.last_capture.clone()
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
        let vision = s.vision_state.lock().unwrap();
        if let Some(mut record) = normalize_tool_record(tool_name, args, &vision) {
            drop(vision);
            if let Some(note) = failure_note.map(str::trim).filter(|n| !n.is_empty()) {
                record.extra_args_hint = Some(format!("FAILED: {note}"));
            }
            s.runtime.last_executed_goal = Some(record.goal.clone());
            s.runtime.push_action(record);
        }
    }

    /// Build [Recent desktop tool calls] for `[CUR_SCREEN]`.
    pub fn recent_actions_prompt_block(&self, conversation_id: &str) -> Option<String> {
        let session = self.get_or_create_session(conversation_id);
        let mut s = session.lock().unwrap();
        s.runtime.auto_close_stale_open_rows();
        format_history_block(s.runtime.history(), s.runtime.give_up_reference())
    }

    /// Legacy hook after assistant round — verify is applied by host pipeline post-execute.
    pub fn on_assistant_round_complete(
        &self,
        _conversation_id: &str,
        _thoughts: Option<&str>,
        _tool_calls: Option<&[ToolCall]>,
    ) {
    }

    /// Reuse the latest pipeline capture, or capture once when no cache exists.
    ///
    /// Used for Verify's before frame. Planning `[CUR_SCREEN]` captures on its own.
    pub async fn ensure_pipeline_capture(
        &self,
        conversation_id: &str,
    ) -> anyhow::Result<ScreenCaptureResult> {
        {
            let session = self.get_or_create_session(conversation_id);
            let s = session.lock().unwrap();
            if let Some(cap) = s.pipeline.cached_capture.clone() {
                log::info!(
                    "pipeline capture: reuse cached frame conversation_id={conversation_id} bytes={}",
                    cap.screenshot_jpeg.len()
                );
                return Ok(cap);
            }
        }
        log::info!("pipeline capture: fresh OS screenshot conversation_id={conversation_id}");
        let (cap, _) = self.capture_screen(conversation_id).await?;
        {
            let session = self.get_or_create_session(conversation_id);
            let mut s = session.lock().unwrap();
            s.pipeline.cached_capture = Some(cap.clone());
        }
        Ok(cap)
    }

    pub fn store_pipeline_after_capture(&self, conversation_id: &str, cap: ScreenCaptureResult) {
        let session = self.get_or_create_session(conversation_id);
        let mut s = session.lock().unwrap();
        s.pipeline.cached_capture = Some(cap);
        s.pipeline.cached_capture_at = Some(std::time::Instant::now());
    }

    /// Latest pipeline capture when it is still younger than `max_age`.
    ///
    /// `[CUR_SCREEN]` calls this before capturing: the verify step of the previous
    /// round already shot exactly this frame, so re-shooting costs a full
    /// `xcap` + resample + jpeg pass for identical pixels. Returns `None` when the
    /// frame is missing or older than `max_age`, in which case the caller captures.
    pub fn fresh_pipeline_capture(
        &self,
        conversation_id: &str,
        max_age: std::time::Duration,
    ) -> Option<ScreenCaptureResult> {
        let session = self.get_or_create_session(conversation_id);
        let s = session.lock().unwrap();
        let stored_at = s.pipeline.cached_capture_at?;
        let age = stored_at.elapsed();
        let cap = s.pipeline.cached_capture.clone()?;
        if age > max_age {
            log::info!(
                "pipeline capture: cached frame too old ({age:?} > {max_age:?}) — recapture"
            );
            return None;
        }
        log::info!(
            "pipeline capture: reuse verify frame for [CUR_SCREEN] bytes={} age={age:?}",
            cap.screenshot_jpeg.len()
        );
        Some(cap)
    }

    pub fn pipeline_context_fields(
        &self,
        conversation_id: &str,
    ) -> (Option<String>, Option<VerifyConclusion>) {
        let session = self.get_or_create_session(conversation_id);
        let s = session.lock().unwrap();
        (
            s.pipeline.last_operation_summary.clone(),
            s.pipeline.last_verify.clone(),
        )
    }

    pub fn apply_pipeline_verify_result(
        &self,
        conversation_id: &str,
        conclusion: &VerifyConclusion,
    ) {
        let parsed = verify_outcome_for_tier(conclusion);
        let outcome = verify_outcome_struct(conclusion);
        let session = self.get_or_create_session(conversation_id);
        let mut s = session.lock().unwrap();
        s.pipeline.last_verify = Some(conclusion.clone());
        let last_goal = s.runtime.last_executed_goal.clone();
        let filled = s.runtime.backfill_batch_open_verify(outcome);
        let backfilled = filled > 0;
        let host_repetition_count =
            crate::agents::computer::runtime::same_goal_repetition_count_in_history(
                s.runtime.history(),
            );
        let config = self.effective_pipeline_config();
        if backfilled {
            s.runtime.on_round_complete(
                &config,
                Some(&parsed),
                last_goal.as_deref(),
                None,
                host_repetition_count,
            );
            log::info!(
                "computer pipeline: verify applied conversation_id={conversation_id} result={}",
                conclusion.action_result.as_history_str()
            );
        } else {
            log::warn!(
                "computer pipeline: verify not backfilled conversation_id={conversation_id} result={}",
                conclusion.action_result.as_history_str()
            );
        }
    }

    /// Close every open history row as `skipped` when Verify will never run.
    ///
    /// Three paths used to leave a row marked `verifying` forever: grounding
    /// failed (nothing was clicked, so there is nothing to judge), the Verify
    /// LLM errored, and the host has verify disabled. `verifying` reads as
    /// "host still judging", and `communication.md` tells the model not to start
    /// a new desktop tool while it sees that — so one leak locks the agent out of
    /// retrying the very step that failed. `skipped` closes the row honestly:
    /// nobody looked at this one.
    ///
    /// Returns the number of rows closed.
    pub fn apply_pipeline_skipped_verify(&self, conversation_id: &str, reason: &str) -> usize {
        let session = self.get_or_create_session(conversation_id);
        let mut s = session.lock().unwrap();
        s.runtime
            .backfill_batch_open_verify(crate::agents::computer::runtime::VerifyOutcome {
                step_result: "skipped".into(),
                cause: Some(reason.to_string()),
            })
    }

    pub fn set_pipeline_last_operation(&self, conversation_id: &str, summary: String) {
        let session = self.get_or_create_session(conversation_id);
        let mut s = session.lock().unwrap();
        s.pipeline.last_operation_summary = Some(summary);
    }

    pub fn next_pipeline_round_seq(&self, conversation_id: &str) -> u32 {
        let session = self.get_or_create_session(conversation_id);
        let mut s = session.lock().unwrap();
        s.pipeline.pipeline_round_seq += 1;
        s.pipeline.pipeline_round_seq
    }

    pub fn current_pipeline_round_seq(&self, conversation_id: &str) -> u32 {
        let session = self.get_or_create_session(conversation_id);
        let s = session.lock().unwrap();
        s.pipeline.pipeline_round_seq
    }

    pub fn tier_history_has_verify_pass(&self, conversation_id: &str) -> bool {
        let session = self.get_or_create_session(conversation_id);
        let s = session.lock().unwrap();
        s.runtime.history().iter().rev().any(|r| {
            r.verify_result
                .as_ref()
                .is_some_and(|v| v.step_result.eq_ignore_ascii_case("pass"))
        })
    }

    pub fn tier_history_has_verify_report(&self, conversation_id: &str) -> bool {
        let session = self.get_or_create_session(conversation_id);
        let s = session.lock().unwrap();
        s.runtime.history().iter().rev().any(|r| {
            r.verify_result.as_ref().is_some_and(|v| {
                let step = v.step_result.as_str();
                step != "skipped" && step != "pending"
            })
        })
    }

    pub fn locked_goal_dynamic_block(&self, conversation_id: &str) -> Option<String> {
        let session = self.get_or_create_session(conversation_id);
        let block = session
            .lock()
            .unwrap()
            .runtime
            .locked_goal_dynamic_block();
        block
    }

    pub fn locked_goal_label(&self, conversation_id: &str) -> Option<String> {
        let session = self.get_or_create_session(conversation_id);
        let guard = session.lock().unwrap();
        guard.runtime.locked_goal_label().map(str::to_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    fn make_state() -> ComputerState {
        ComputerState::default_for_test()
    }

    /// The pipeline cache holds exactly one frame: a round stores its after-frame
    /// and the next round reads it back as the before frame.
    ///
    /// This is the only test for that round-trip. An integration-test copy in
    /// `tests/computer_close_loop.rs` used to exist; it went stale when the
    /// before frame moved off `ScreenCaptureResult`, and when rewritten against
    /// the cache API it hung on macOS (`fresh_pipeline_capture` returned once,
    /// then blocked on the second call inside an integration-test binary). This
    /// unit test exercises the same functions and finishes in a fraction of a
    /// second, so the integration copy was removed rather than kept as a hang.
    /// 定位阶段：配置了强度就用，默认 `off` 表示关闭。
    #[test]
    fn locate_phase_applies_configured_thinking_intensity() {
        let mut state = make_state();
        state.pipeline_config.pipeline_llm.thinking_intensity_locate = Some("high".into());
        let mut s = crate::models::ModelSettings::default();
        state.apply_pipeline_thinking(PipelineLlmPhase::Locate, &mut s);
        assert!(s.round_thinking_locked);
        assert_eq!(s.round_enable_thinking, Some(true));
        // `high` maps to the same 4096 a provider that speaks budgets expects.
        assert_eq!(s.round_thinking_budget, Some(4096));
        assert_eq!(s.round_thinking_intensity.as_deref(), Some("high"));

        let state = make_state();
        let mut s = crate::models::ModelSettings::default();
        state.apply_pipeline_thinking(PipelineLlmPhase::Locate, &mut s);
        assert_eq!(s.round_enable_thinking, Some(false));
        assert_eq!(s.round_thinking_budget, None);
        assert_eq!(s.round_thinking_intensity.as_deref(), Some("off"));
    }

    #[test]
    fn ensure_pipeline_capture_reuses_stored_frame() {
        let state = make_state();
        let cap = ScreenCaptureResult {
            raw_unmarked_jpeg: vec![7],
            screenshot_jpeg: vec![9, 9, 9],
            monitor: screen::MonitorInfo::new(0, 0, 100, 100),
        };
        state.store_pipeline_after_capture("reuse", cap);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let got = rt
            .block_on(state.ensure_pipeline_capture("reuse"))
            .expect("reuse");
        assert_eq!(got.screenshot_jpeg, vec![9, 9, 9]);
    }

    #[test]
    fn fresh_pipeline_capture_returns_stored_frame_within_age() {
        let state = make_state();
        let cap = ScreenCaptureResult {
            raw_unmarked_jpeg: vec![1],
            screenshot_jpeg: vec![9, 9, 9],
            monitor: screen::MonitorInfo::new(0, 0, 100, 100),
        };
        state.store_pipeline_after_capture("fresh", cap);

        let got = state
            .fresh_pipeline_capture("fresh", Duration::from_secs(120))
            .expect("frame is young enough");
        assert_eq!(got.screenshot_jpeg, vec![9, 9, 9]);
    }

    /// Full round-trip on a real screenshot, with no OS capture anywhere.
    ///
    /// `apply_screen_capture` takes the pixels as an argument, so the whole
    /// pipeline — JPEG decode, pointer overlay, cache store — runs on a fixture
    /// image. That is the shape every pipeline test should have: the only thing
    /// that must never happen under `cargo test` is a live `xcap` grab, and
    /// nothing here can reach one.
    #[test]
    fn real_screenshot_round_trips_through_the_pipeline_cache() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/locate_fixture/fixtures/sudoku.jpg");
        let jpeg = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let (w, h) = image::ImageReader::new(std::io::Cursor::new(&jpeg))
            .with_guessed_format()
            .expect("probe")
            .into_dimensions()
            .expect("dimensions");

        let state = make_state();
        let monitor = screen::MonitorInfo::new(0, 0, w as i32, h as i32);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        let cap = rt
            .block_on(state.apply_screen_capture(
                "real",
                &jpeg,
                monitor,
                (w, h),
                (100, 100),
                None,
            ))
            .expect("pipeline ran on the fixture image");

        // The unmarked copy is the raw input, untouched.
        assert_eq!(
            cap.raw_unmarked_jpeg.as_slice(),
            jpeg.as_slice(),
            "raw_unmarked_jpeg must be the input bytes, not a re-encode"
        );
        assert!(
            !cap.screenshot_jpeg.is_empty(),
            "the overlay pass produced no image"
        );

        state.store_pipeline_after_capture("real", cap.clone());
        let reused = state
            .fresh_pipeline_capture("real", Duration::from_secs(120))
            .expect("cached frame is fresh");
        assert_eq!(
            reused.screenshot_jpeg.as_slice(),
            cap.screenshot_jpeg.as_slice(),
            "the next round must read back exactly what this round produced"
        );
    }

    #[test]
    fn fresh_pipeline_capture_rejects_stale_frame() {
        let state = make_state();
        let cap = ScreenCaptureResult {
            raw_unmarked_jpeg: vec![1],
            screenshot_jpeg: vec![9, 9, 9],
            monitor: screen::MonitorInfo::new(0, 0, 100, 100),
        };
        state.store_pipeline_after_capture("stale", cap);

        // Zero max age: any elapsed time is already too old.
        assert!(
            state
                .fresh_pipeline_capture("stale", Duration::from_millis(0))
                .is_none(),
            "a frame past its max age must not be reused"
        );
    }

    #[test]
    fn fresh_pipeline_capture_none_without_store() {
        let state = make_state();
        assert!(
            state
                .fresh_pipeline_capture("empty", Duration::from_secs(120))
                .is_none(),
            "no stored frame means the caller must capture"
        );
    }

    #[test]
    fn fresh_pipeline_capture_does_not_consume_cache() {
        let state = make_state();
        let cap = ScreenCaptureResult {
            raw_unmarked_jpeg: vec![1],
            screenshot_jpeg: vec![9, 9, 9],
            monitor: screen::MonitorInfo::new(0, 0, 100, 100),
        };
        state.store_pipeline_after_capture("keep", cap);

        // [CUR_SCREEN] reads it, then verify's `ensure_pipeline_capture` needs the
        // same frame as its before image — so reading must not clear the slot.
        assert!(state
            .fresh_pipeline_capture("keep", Duration::from_secs(120))
            .is_some());
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let got = rt
            .block_on(state.ensure_pipeline_capture("keep"))
            .expect("verify before frame");
        assert_eq!(got.screenshot_jpeg, vec![9, 9, 9]);
    }

    #[test]
    fn session_create_and_reuse() {
        let state = make_state();
        let s1 = state.vision_state_for_conversation("a");
        let s2 = state.vision_state_for_conversation("a");
        assert!(
            Arc::ptr_eq(&s1, &s2),
            "same conversation_id returns same session"
        );
    }

    #[test]
    fn distinct_sessions_are_isolated() {
        let state = make_state();
        let sa = state.vision_state_for_conversation("a");
        let sb = state.vision_state_for_conversation("b");
        assert!(
            !Arc::ptr_eq(&sa, &sb),
            "different conversation_id must yield different sessions"
        );
    }

    #[test]
    fn mark_ended_and_cancelled_status() {
        let state = make_state();
        state.mark_ended("x");
        assert_eq!(
            state.get_or_create_session("x").lock().unwrap().status,
            SessionStatus::Ended
        );

        state.mark_cancelled("y");
        assert_eq!(
            state.get_or_create_session("y").lock().unwrap().status,
            SessionStatus::Cancelled
        );
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
        assert!(
            ended_before <= 1,
            "at most one ended session should survive after evicting ended first"
        );
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
        assert_eq!(
            state
                .get_or_create_session("a")
                .lock()
                .unwrap()
                .selected_monitor
                .as_deref(),
            Some("m1")
        );
        assert!(state
            .get_or_create_session("b")
            .lock()
            .unwrap()
            .selected_monitor
            .is_none());
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
        _profile: MouseMoveProfile,
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
    fn key_phase(
        &self,
        _name: &str,
        _phase: actions::KeyPhase,
    ) -> anyhow::Result<actions::ActionResult> {
        anyhow::bail!("enigo backend not available; computer actions are disabled")
    }
    fn mouse_phase(
        &self,
        _button: actions::MouseButton,
        _phase: actions::KeyPhase,
    ) -> anyhow::Result<actions::ActionResult> {
        anyhow::bail!("enigo backend not available; computer actions are disabled")
    }
}
