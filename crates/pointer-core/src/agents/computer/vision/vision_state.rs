//! Screen-resolved overlay state. Bitmap-space boxes ([`super::annotate::BoxInfo`], [`super::annotate::IndexMap`]) are converted to [`ElementInfo`] via [`VisionState::set_index_map_from_boxes`].

use super::annotate::BoxInfo;
use super::coord::CoordinateSystem;
use super::screen::MonitorInfo;
use serde_json::Value;
use std::collections::HashMap;
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
    pub fn new(tool_name: impl Into<String>, method: impl Into<String>, args: impl Into<String>) -> Self {
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
    index_map: HashMap<u32, ElementInfo>,
    /// Current monitor info.
    screen_bbox: Option<MonitorInfo>,
    /// Current coordinate system.
    coordinate_system: CoordinateSystem,
    /// History of recent actions.
    action_history: Vec<RecentAction>,
    /// Maximum number of actions to keep in history.
    max_history_size: usize,
}

/// Corner anchor for index + offset positioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CornerAnchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

impl CornerAnchor {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "top-left" | "topleft" => Some(Self::TopLeft),
            "top-right" | "topright" => Some(Self::TopRight),
            "bottom-left" | "bottomleft" => Some(Self::BottomLeft),
            "bottom-right" | "bottomright" => Some(Self::BottomRight),
            "center" | "center-point" => Some(Self::Center),
            _ => None,
        }
    }
}

/// Information about a UI element extracted from annotation.
#[derive(Debug, Clone)]
pub struct ElementInfo {
    /// The annotated index.
    pub index: u32,
    /// Center X coordinate in screen pixels.
    pub center_x: i32,
    /// Center Y coordinate in screen pixels.
    pub center_y: i32,
    /// Width of the element.
    pub width: f32,
    /// Height of the element.
    pub height: f32,
    /// Session-normalized bbox (0–1000).
    pub norm_left: i32,
    pub norm_top: i32,
    pub norm_right: i32,
    pub norm_bottom: i32,
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

    /// Set the index map from annotated boxes.
    ///
    /// `boxes` are in **bitmap pixel** space (same as the screenshot sent to annotate). `monitor`
    /// uses **logical** bounds (e.g. macOS `CGDisplayBounds`). When the capture bitmap is larger
    /// than the logical size (Retina), pass its pixel size as `capture_px` so centers match
    /// enigo / OS global pointer coordinates.
    ///
    /// # Arguments
    /// * `boxes` - Detected bounding boxes from the annotation service.
    /// * `monitor` - Logical monitor bounds (global `left`/`top` plus `width`/`height`).
    /// * `capture_px` - Width and height of the captured bitmap in pixels.
    pub fn set_index_map_from_boxes(
        &mut self,
        boxes: &[BoxInfo],
        monitor: &MonitorInfo,
        capture_px: (u32, u32),
    ) {
        let mw = monitor.width.max(1) as f32;
        let mh = monitor.height.max(1) as f32;
        let sx = capture_px.0 as f32 / mw;
        let sy = capture_px.1 as f32 / mh;

        self.index_map = boxes
            .iter()
            .map(|b| {
                let (cx, cy) = b.center();
                let lx = (cx / sx).round() as i32;
                let ly = (cy / sy).round() as i32;
                let px_left = monitor.left + (b.x / sx).round() as i32;
                let px_top = monitor.top + (b.y / sy).round() as i32;
                let px_right = monitor.left + ((b.x + b.width) / sx).round() as i32;
                let px_bottom = monitor.top + ((b.y + b.height) / sy).round() as i32;
                let (nl, nt) = Self::screen_to_session_pair(px_left, px_top, monitor);
                let (nr, nb) = Self::screen_to_session_pair(px_right, px_bottom, monitor);
                (
                    b.index,
                    ElementInfo {
                        index: b.index,
                        center_x: monitor.left + lx,
                        center_y: monitor.top + ly,
                        width: b.width / sx,
                        height: b.height / sy,
                        norm_left: nl,
                        norm_top: nt,
                        norm_right: nr,
                        norm_bottom: nb,
                    },
                )
            })
            .collect();
    }

    fn screen_to_session_pair(px: i32, py: i32, monitor: &MonitorInfo) -> (i32, i32) {
        let (nx, ny) = super::coord::screen_to_normalized((px, py), monitor, CoordinateSystem::Qwen);
        (nx.round() as i32, ny.round() as i32)
    }

    /// Set the index map directly.
    pub fn set_index_map(&mut self, index_map: HashMap<u32, ElementInfo>) {
        self.index_map = index_map;
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

    pub fn resolve_index(&self, index: u32) -> Option<(i32, i32)> {
        self.index_map.get(&index).map(|e| (e.center_x, e.center_y))
    }

    pub fn element(&self, index: u32) -> Option<&ElementInfo> {
        self.index_map.get(&index)
    }

    /// Index + corner anchor + session-coordinate delta → screen pixels.
    pub fn resolve_index_anchor(
        &self,
        index: u32,
        anchor: CornerAnchor,
        delta_x: i32,
        delta_y: i32,
    ) -> Option<(i32, i32)> {
        let e = self.index_map.get(&index)?;
        let monitor = self.screen_bbox.as_ref()?;
        let (nx, ny) = match anchor {
            CornerAnchor::TopLeft => (e.norm_left, e.norm_top),
            CornerAnchor::TopRight => (e.norm_right, e.norm_top),
            CornerAnchor::BottomLeft => (e.norm_left, e.norm_bottom),
            CornerAnchor::BottomRight => (e.norm_right, e.norm_bottom),
            CornerAnchor::Center => {
                ((e.norm_left + e.norm_right) / 2, (e.norm_top + e.norm_bottom) / 2)
            }
        };
        let (sx, sy) = super::coord::normalized_to_screen(
            ((nx + delta_x) as f32, (ny + delta_y) as f32),
            monitor,
            self.coordinate_system,
        );
        Some((sx, sy))
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

    /// Get the index map.
    pub fn index_map(&self) -> &HashMap<u32, ElementInfo> {
        &self.index_map
    }

    /// Clear all state.
    pub fn clear(&mut self) {
        self.index_map.clear();
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
        let goal = args.get("goal").and_then(|v| v.as_str()).unwrap_or("").trim();
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
            return format!("(fields: {})", keys.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(","));
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
    fn test_resolve_index() {
        let mut state = VisionState::new();
        let mut index_map = HashMap::new();
        index_map.insert(
            1,
            ElementInfo {
                index: 1,
                center_x: 100,
                center_y: 200,
                width: 50.0,
                height: 30.0,
                norm_left: 0,
                norm_top: 0,
                norm_right: 100,
                norm_bottom: 100,
            },
        );
        state.set_index_map(index_map);

        assert_eq!(state.resolve_index(1), Some((100, 200)));
        assert_eq!(state.resolve_index(99), None);
    }

    #[test]
    fn test_resolve_coordinate() {
        let mut state = VisionState::new();
        state.set_screen_bbox(MonitorInfo::new(0, 0, 1920, 1080));
        state.set_coordinate_system(CoordinateSystem::Qwen);

        let pos = state.resolve_coordinate(500.0, 500.0);
        assert_eq!(pos, Some((960, 540)));
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
        let mut index_map = HashMap::new();
        index_map.insert(
            1,
            ElementInfo {
                index: 1,
                center_x: 100,
                center_y: 200,
                width: 50.0,
                height: 30.0,
                norm_left: 0,
                norm_top: 0,
                norm_right: 100,
                norm_bottom: 100,
            },
        );
        state.set_index_map(index_map);
        state.set_screen_bbox(MonitorInfo::new(0, 0, 1920, 1080));
        state.record_action(RecentAction::new("mouse", "click", "{}"));

        state.clear();
        assert!(state.index_map().is_empty());
        assert!(state.action_history().is_empty());
        assert_eq!(state.resolve_coordinate(500.0, 500.0), None);
    }

    #[test]
    fn test_set_index_map_from_boxes() {
        let mut state = VisionState::new();
        let boxes = vec![
            BoxInfo {
                index: 1,
                x: 100.0,
                y: 200.0,
                width: 50.0,
                height: 30.0,
                confidence: 0.95,
            },
            BoxInfo {
                index: 2,
                x: 300.0,
                y: 400.0,
                width: 60.0,
                height: 40.0,
                confidence: 0.90,
            },
        ];
        let monitor = MonitorInfo::new(0, 0, 1920, 1080);
        state.set_index_map_from_boxes(&boxes, &monitor, (1920, 1080));

        assert_eq!(state.resolve_index(1), Some((125, 215)));
        assert_eq!(state.resolve_index(2), Some((330, 420)));
    }

    #[test]
    fn test_set_index_map_from_boxes_retina_scale() {
        let mut state = VisionState::new();
        let boxes = vec![BoxInfo {
            index: 1,
            x: 200.0,
            y: 400.0,
            width: 100.0,
            height: 80.0,
            confidence: 1.0,
        }];
        let monitor = MonitorInfo::new(0, 0, 1000, 800);
        state.set_index_map_from_boxes(&boxes, &monitor, (2000, 1600));

        assert_eq!(state.resolve_index(1), Some((125, 220)));
    }
}
