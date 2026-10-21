//! Screen-resolved overlay state: monitor bbox, coordinate system, and action history.

use super::coord::CoordinateSystem;
use super::screen::MonitorInfo;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// Information about a recently executed action.
#[derive(Debug, Clone)]
pub struct RecentAction {
    /// Name of the tool that was executed.
    pub tool_name: String,
    /// Method called within the tool.
    pub method: String,
    /// Arguments passed to the method.
    pub args: String,
    /// Timestamp when the action was executed.
    pub timestamp_ms: u64,
}

impl RecentAction {
    /// Create a new RecentAction.
    pub fn new(
        tool_name: impl Into<String>,
        method: impl Into<String>,
        args: impl Into<String>,
    ) -> Self {
        Self {
            tool_name: tool_name.into(),
            method: method.into(),
            args: args.into(),
            timestamp_ms: current_timestamp_ms(),
        }
    }
}

/// Manages the visual state for computer use operations.
///
/// This struct holds the index map (annotated elements), screen info,
/// coordinate system, and action history for a single turn.
#[derive(Debug, Clone, Default)]
pub struct VisionState {
    /// Mapping from annotated index to element info.
    /// Current monitor info.
    screen_bbox: Option<MonitorInfo>,
    /// Current coordinate system.
    coordinate_system: CoordinateSystem,
    /// Crop window for the current `[Zoom pointer after action]`, if injected.
    /// History of recent actions.
    action_history: Vec<RecentAction>,
    /// Maximum number of actions to keep in history.
    max_history_size: usize,
}

impl VisionState {
    /// Create a new VisionState with default settings.
    pub fn new() -> Self {
        Self {
            max_history_size: 10,
            coordinate_system: CoordinateSystem::Qwen,
            ..Default::default()
        }
    }

    /// Create a new VisionState with a custom history size.
    pub fn with_history_size(max_history_size: usize) -> Self {
        Self {
            max_history_size,
            coordinate_system: CoordinateSystem::Qwen,
            ..Default::default()
        }
    }

    /// Set the screen bounding box.
    pub fn set_screen_bbox(&mut self, monitor: MonitorInfo) {
        self.screen_bbox = Some(monitor);
    }

    /// Set the coordinate system.
    pub fn set_coordinate_system(&mut self, system: CoordinateSystem) {
        self.coordinate_system = system;
    }

    /// Get the current coordinate system.
    pub fn coordinate_system(&self) -> CoordinateSystem {
        self.coordinate_system
    }

    /// Resolve an annotated element index to screen pixel coordinates.
    ///
    /// This is the **index-based** positioning path.
    ///
    /// # Arguments
    /// * `index` - The annotated index number.
    ///
    /// # Returns
    /// Some((x, y)) if the index exists, None otherwise.
    pub fn screen_bbox(&self) -> Option<MonitorInfo> {
        self.screen_bbox
    }

    /// Resolve normalized coordinates to screen pixel coordinates.
    ///
    /// This is the **coordinate-based** positioning path.
    ///
    /// # Arguments
    /// * `x` - Normalized X coordinate.
    /// * `y` - Normalized Y coordinate.
    ///
    /// # Returns
    /// Some((x, y)) if screen bbox is set, None otherwise.
    pub fn resolve_coordinate(&self, x: f32, y: f32) -> Option<(i32, i32)> {
        let monitor = self.screen_bbox?;
        Some(super::coord::normalized_to_screen(
            (x, y),
            &monitor,
            self.coordinate_system,
        ))
    }

    /// Record an action in the history.
    pub fn record_action(&mut self, action: RecentAction) {
        self.action_history.push(action);
        if self.action_history.len() > self.max_history_size {
            self.action_history.remove(0);
        }
    }

    /// Get the action history.
    pub fn action_history(&self) -> &[RecentAction] {
        &self.action_history
    }

    /// Clear all state.
    pub fn clear(&mut self) {
        self.screen_bbox = None;
        self.action_history.clear();
    }

    /// Append a desktop tool row for repetition analysis (uses **`goal`** / **`action`** text when present).
    /// On failure, `failed_note` is appended as ` | FAILED: …` so ineffective repeats are visible next to `[CUR_SCREEN]`.
    pub fn record_desktop_tool_invocation(
        &mut self,
        tool_name: &str,
        args: &Value,
        failed_note: Option<&str>,
    ) {
        let method = args
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let goal = args
            .get("goal")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let method_line = if !method.is_empty() {
            method.to_string()
        } else {
            match tool_name {
                "hotkey" => "hotkey".into(),
                "wait" => "wait".into(),
                _ => "-".into(),
            }
        };
        let mut summary = if goal.is_empty() && action.is_empty() {
            compact_args_hint(args)
        } else {
            format!("goal={goal} | action={action}")
        };
        if let Some(note) = failed_note.map(str::trim).filter(|s| !s.is_empty()) {
            summary.push_str(" | FAILED: ");
            summary.push_str(&truncate_failed_note(note, 220));
        }
        self.record_action(RecentAction::new(tool_name, method_line, summary));
    }

    /// Up to five recent rows for injection under `[CUR_SCREEN]` (oldest → newest).
    pub fn recent_actions_prompt_block(&self) -> Option<String> {
        let history = self.action_history();
        if history.is_empty() {
            return None;
        }
        let take = history.len().min(5);
        let slice = &history[history.len() - take..];
        let mut lines = vec!["[Recent desktop tool calls — ordered oldest to newest; use goal/action to spot repeated ineffective attempts; failed rows end with FAILED: …; overlay indices here are not comparable across turns.]".to_string()];
        for (i, a) in slice.iter().enumerate() {
            lines.push(format!(
                "{}. {}:{} — {}",
                i + 1,
                a.tool_name,
                a.method,
                a.args
            ));
        }
        Some(lines.join("\n"))
    }
}

fn truncate_failed_note(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max_chars).collect::<String>())
    }
}

fn compact_args_hint(args: &Value) -> String {
    if let Some(obj) = args.as_object() {
        let keys: Vec<&String> = obj.keys().take(8).collect();
        if !keys.is_empty() {
            return format!(
                "(fields: {})",
                keys.iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            );
        }
    }
    args.to_string()
}

fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_coordinate() {
        let mut state = VisionState::new();
        state.set_screen_bbox(MonitorInfo::new(0, 0, 1920, 1080));
        state.set_coordinate_system(CoordinateSystem::Qwen);

        let pos = state.resolve_coordinate(500.0, 500.0);
        assert_eq!(pos, Some((960, 540)));
    }

    /// The `mouse_click_at` chain: model point -> session -> screen.
    /// A pixel value the model supplies must land on that same pixel.
    #[test]
    fn pixel_point_flows_to_screen_without_drift() {
        let mut state = VisionState::new();
        state.set_screen_bbox(MonitorInfo::new(0, 0, 1920, 1080));
        state.set_coordinate_system(CoordinateSystem::Pixel);
        for (x, y) in [(0.0, 0.0), (820.0, 45.0), (1919.0, 1079.0)] {
            let (px, py) = state.resolve_coordinate(x, y).expect("resolved");
            assert_eq!((px, py), (x as i32, y as i32), "point {x},{y}");
        }
    }

    /// Same chain on a secondary monitor: the monitor origin is added once.
    #[test]
    fn pixel_point_flows_to_screen_on_secondary_monitor() {
        let mut state = VisionState::new();
        state.set_screen_bbox(MonitorInfo::new(1920, 0, 1920, 1080));
        state.set_coordinate_system(CoordinateSystem::Pixel);
        let (px, py) = state.resolve_coordinate(820.0, 45.0).expect("resolved");
        assert_eq!((px, py), (2740, 45));
    }

    #[test]
    fn test_resolve_coordinate_without_bbox() {
        let state = VisionState::new();
        assert_eq!(state.resolve_coordinate(500.0, 500.0), None);
    }

    #[test]
    fn test_action_history() {
        let mut state = VisionState::with_history_size(3);
        state.record_action(RecentAction::new("mouse", "click", "{\"index\":1}"));
        state.record_action(RecentAction::new("mouse", "click", "{\"index\":2}"));
        state.record_action(RecentAction::new("mouse", "click", "{\"index\":3}"));
        state.record_action(RecentAction::new("mouse", "click", "{\"index\":4}"));

        assert_eq!(state.action_history().len(), 3);
        assert_eq!(state.action_history()[0].args, "{\"index\":2}");
    }

    #[test]
    fn recent_actions_prompt_block_orders_and_caps_at_five() {
        let mut state = VisionState::new();
        for i in 0..7 {
            state.record_desktop_tool_invocation(
                "mouse",
                &serde_json::json!({
                    "method": "click_index",
                    "goal": format!("g{i}"),
                    "action": format!("a{i}"),
                }),
                None,
            );
        }
        let block = state.recent_actions_prompt_block().expect("block");
        assert!(block.contains("[Recent desktop tool calls"));
        assert!(block.contains("g2"));
        assert!(!block.contains("g0"));
        assert!(block.contains("g6"));
    }

    #[test]
    fn recent_actions_prompt_includes_failed_suffix() {
        let mut state = VisionState::new();
        state.record_desktop_tool_invocation(
            "mouse",
            &serde_json::json!({
                "method": "click_index",
                "goal": "open menu",
                "action": "tap file",
            }),
            Some("no such index"),
        );
        let block = state.recent_actions_prompt_block().expect("block");
        assert!(block.contains("FAILED:"));
        assert!(block.contains("open menu"));
        assert!(block.contains("no such index"));
    }

    #[test]
    fn test_clear() {
        let mut state = VisionState::new();
        state.set_screen_bbox(MonitorInfo::new(0, 0, 1920, 1080));
        state.record_action(RecentAction::new("mouse", "click", "{}"));

        state.clear();
        assert!(state.action_history().is_empty());
        assert_eq!(state.resolve_coordinate(500.0, 500.0), None);
    }

}
