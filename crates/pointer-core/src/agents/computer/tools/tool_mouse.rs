use super::args_util::{
    click_count_from_args, human_like_from_args, mouse_button_from_args, require_non_empty_str,
    required_f32_arg, scroll_pixels_from_args,
};
use super::method_route::MouseBackend;
use crate::agents::computer::control::ComputerControl;
use crate::agents::computer::actions::ActionExecutor;
use crate::agents::computer::verify::VerifyHintGenerator;
use crate::agents::computer::vision_state::VisionState;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::{Arc, Mutex};

// ── MouseAtTool ──────────────────────────────────────────────────────────────

/// Mouse actions using logical screen pixel coordinates.
pub struct MouseAtTool {
    executor: Arc<Mutex<ActionExecutor>>,
    /// L1 abstraction for the stateless actions. `drag_to` keeps using the
    /// executor because it honours a per-call `human_like` override that the
    /// trait does not carry.
    control: Arc<dyn ComputerControl>,
    vision_state: Arc<Mutex<VisionState>>,
    verify: VerifyHintGenerator,
    human_like_default: bool,
}

impl MouseAtTool {
    pub fn new(
        executor: Arc<Mutex<ActionExecutor>>,
        control: Arc<dyn ComputerControl>,
        vision_state: Arc<Mutex<VisionState>>,
        human_like_default: bool,
    ) -> Self {
        Self {
            executor,
            control,
            vision_state,
            verify: VerifyHintGenerator::new(),
            human_like_default,
        }
    }

    fn human_like(&self, args: &Value) -> bool {
        human_like_from_args(args, self.human_like_default)
    }

    pub fn execute(&self, method: &str, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        match method {
            "click" => self.click(args),
            "double_click" => self.double_click(args),
            "right_click" => self.right_click(args),
            "hover" => self.hover(args),
            "drag_from_to" => self.drag_from_to(args),
            "mouse_down" => self.mouse_down(args),
            "mouse_up" => self.mouse_up(args),
            "drag_to" => self.drag_to(args),
            _ => Err(anyhow!(
                "Unknown mouse_at method: {method}. Use click, double_click, right_click, hover, drag_from_to, mouse_down, mouse_up, drag_to."
            )),
        }
    }

    fn resolve_coord(&self, _args: &Value, x: f32, y: f32) -> Result<(i32, i32)> {
        let vision = self.vision_state.lock().unwrap();
        vision
            .resolve_coordinate(x, y)
            .ok_or_else(|| anyhow!("Screen bounds not set"))
    }

    fn click(&self, args: &Value) -> Result<String> {
        let x = required_f32_arg(args, "x")?;
        let y = required_f32_arg(args, "y")?;
        let (px, py) = self.resolve_coord(args, x, y)?;
        let button = mouse_button_from_args(args)?;
        let clicks = click_count_from_args(args)?;
        self.control.click(button, Some(px), Some(py), clicks)?;
        Ok(self.verify.click_hint(None, Some((px, py))))
    }

    fn double_click(&self, args: &Value) -> Result<String> {
        let x = required_f32_arg(args, "x")?;
        let y = required_f32_arg(args, "y")?;
        let (px, py) = self.resolve_coord(args, x, y)?;
        self.control.double_click(px, py)?;
        Ok(self.verify.click_hint(None, Some((px, py))))
    }

    fn right_click(&self, args: &Value) -> Result<String> {
        let x = required_f32_arg(args, "x")?;
        let y = required_f32_arg(args, "y")?;
        let (px, py) = self.resolve_coord(args, x, y)?;
        // Compatibility alias for `mouse_click_at` with `button: right`.
        self.control.right_click(px, py)?;
        Ok(self.verify.click_hint(None, Some((px, py))))
    }

    fn hover(&self, args: &Value) -> Result<String> {
        let x = required_f32_arg(args, "x")?;
        let y = required_f32_arg(args, "y")?;
        let (px, py) = self.resolve_coord(args, x, y)?;
        self.control.move_to(px, py)?;
        Ok(format!("Hovered at ({}, {}) px", px, py))
    }

    fn drag_from_to(&self, args: &Value) -> Result<String> {
        let x1 = required_f32_arg(args, "x1")
            .map_err(|_| anyhow!("drag_from_to requires x1, y1, x2, y2"))?;
        let y1 = required_f32_arg(args, "y1").map_err(|_| anyhow!("drag_from_to requires y1"))?;
        let x2 = required_f32_arg(args, "x2").map_err(|_| anyhow!("drag_from_to requires x2"))?;
        let y2 = required_f32_arg(args, "y2").map_err(|_| anyhow!("drag_from_to requires y2"))?;
        let vision = self.vision_state.lock().unwrap();
        let (px1, py1) = vision
            .resolve_coordinate(x1, y1)
            .ok_or_else(|| anyhow!("Screen bounds not set"))?;
        let (px2, py2) = vision
            .resolve_coordinate(x2, y2)
            .ok_or_else(|| anyhow!("Screen bounds not set"))?;
        drop(vision);
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.drag_left_from_to(px1, py1, px2, py2, hl)?;
        Ok(format!(
            "Dragged ({}, {}) -> ({}, {}). Verify on next screenshot.",
            px1, py1, px2, py2
        ))
    }

    /// `DRAG_TO` — drag from the **current** pointer to `(x, y)`.
    fn drag_to(&self, args: &Value) -> Result<String> {
        let x = required_f32_arg(args, "x")?;
        let y = required_f32_arg(args, "y")?;
        let (px, py) = self.resolve_coord(args, x, y)?;
        self.control.drag_to(px, py)?;
        Ok(format!(
            "Dragged from the current pointer to ({}, {}). Verify on next screenshot.",
            px, py
        ))
    }

    /// `MOUSE_DOWN` — hold `button` at the current pointer.
    fn mouse_down(&self, args: &Value) -> Result<String> {
        let button = mouse_button_from_args(args)?;
        self.control.mouse_down(button)?;
        Ok(format!(
            "Held {} button down. Release with mouse_up before the next click.",
            button.as_str()
        ))
    }

    /// `MOUSE_UP` — release `button` at the current pointer.
    fn mouse_up(&self, args: &Value) -> Result<String> {
        let button = mouse_button_from_args(args)?;
        self.control.mouse_up(button)?;
        Ok(format!("Released {} button.", button.as_str()))
    }
}

/// `mouse_scroll_current` — scroll at the current pointer by **pixels**.
///
/// Positive `dy` scrolls up, positive `dx` scrolls right. The local backend
/// converts pixels to wheel lines; the remote backend forwards them to
/// `pyautogui.hscroll` / `vscroll` unchanged.
pub fn scroll_current(executor: &Arc<Mutex<ActionExecutor>>, args: &Value) -> Result<String> {
    require_non_empty_str(args, "goal")?;
    let (dx, dy) = scroll_pixels_from_args(args)?;
    let executor = executor.lock().unwrap();
    executor.scroll_at_current_pixels(dx, dy)?;
    Ok(format!("Scrolled ({dx}, {dy}) px at the current pointer."))
}

// ── MouseTool (unified registry entry) ───────────────────────────────────────

/// Unified mouse tool: routes `click_at` / `click_index` etc. by method name.
pub struct MouseTool {
    at: MouseAtTool,
}

impl MouseTool {
    pub fn new(
        executor: Arc<Mutex<ActionExecutor>>,
        control: Arc<dyn ComputerControl>,
        vision_state: Arc<Mutex<VisionState>>,
        human_like_default: bool,
    ) -> Self {
        Self {
            at: MouseAtTool::new(executor, control, vision_state, human_like_default),
        }
    }

    #[allow(dead_code)]
    pub fn execute(&self, args: &Value) -> Result<String> {
        use super::method_route::{route_mouse, MouseBackend};
        let routed = route_mouse(args)?;
        log::info!(
            "mouse: method={} backend={:?}",
            routed.method,
            routed.backend
        );
        match routed.backend {
            MouseBackend::At => self.at.execute(&routed.method, args),
        }
    }

    /// Direct dispatch for flat tool names — bypasses `method`-based routing.
    pub fn execute_with(
        &self,
        backend: MouseBackend,
        method: &str,
        args: &Value,
    ) -> Result<String> {
        match backend {
            MouseBackend::At => self.at.execute(method, args),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::computer::actions::{ActionBackend, ActionResult, MouseButton};
    use crate::agents::computer::input::actions::KeyPhase;
    use crate::agents::computer::input::mouse_move::MouseMoveProfile;
    use crate::agents::computer::vision::coord::CoordinateSystem;
    use crate::agents::computer::vision::screen::MonitorInfo;
    use crate::agents::computer::vision_state::VisionState;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingBackend {
        calls: Arc<Mutex<Vec<String>>>,
        position: Arc<Mutex<(i32, i32)>>,
    }

    impl RecordingBackend {
        fn record(&self, call: impl Into<String>) {
            self.calls.lock().unwrap().push(call.into());
        }
    }

    impl ActionBackend for RecordingBackend {
        fn click(&self) -> Result<ActionResult> {
            self.record("click");
            Ok(ActionResult::success("click"))
        }
        fn double_click(&self) -> Result<ActionResult> {
            self.record("double_click");
            Ok(ActionResult::success("double_click"))
        }
        fn right_click(&self) -> Result<ActionResult> {
            self.record("right_click");
            Ok(ActionResult::success("right_click"))
        }
        fn move_to_with_profile(
            &self,
            x: i32,
            y: i32,
            _profile: MouseMoveProfile,
        ) -> Result<ActionResult> {
            self.record(format!("move_to({x},{y})"));
            *self.position.lock().unwrap() = (x, y);
            Ok(ActionResult::success("move_to"))
        }
        fn scroll(&self, lines: i32) -> Result<ActionResult> {
            self.record(format!("scroll({lines})"));
            Ok(ActionResult::success("scroll"))
        }
        fn scroll_horizontal(&self, lines: i32) -> Result<ActionResult> {
            self.record(format!("scroll_horizontal({lines})"));
            Ok(ActionResult::success("scroll_horizontal"))
        }
        fn type_text(&self, text: &str) -> Result<ActionResult> {
            self.record(format!("type_text({text})"));
            Ok(ActionResult::success("type_text"))
        }
        fn hotkey(&self, keys: &[&str]) -> Result<ActionResult> {
            self.record(format!("hotkey({})", keys.join("+")));
            Ok(ActionResult::success("hotkey"))
        }
        fn get_position(&self) -> Result<(i32, i32)> {
            Ok(*self.position.lock().unwrap())
        }
        fn key_phase(&self, name: &str, phase: KeyPhase) -> Result<ActionResult> {
            let p = match phase {
                KeyPhase::Press => "down",
                KeyPhase::Release => "up",
            };
            self.record(format!("key_{p}({name})"));
            Ok(ActionResult::success("key_phase"))
        }
        fn mouse_phase(&self, button: MouseButton, phase: KeyPhase) -> Result<ActionResult> {
            let p = match phase {
                KeyPhase::Press => "down",
                KeyPhase::Release => "up",
            };
            self.record(format!("mouse_{p}({})", button.as_str()));
            Ok(ActionResult::success("mouse_phase"))
        }
    }

    fn tool() -> (MouseTool, Arc<Mutex<Vec<String>>>) {
        let backend = RecordingBackend::default();
        let calls = backend.calls.clone();
        let executor = Arc::new(Mutex::new(ActionExecutor::new(Box::new(backend))));
        let mut vision = VisionState::new();
        vision.set_coordinate_system(CoordinateSystem::Pixel);
        vision.set_screen_bbox(MonitorInfo::new(0, 0, 1280, 800));
        let vision = Arc::new(Mutex::new(vision));
        let control: Arc<dyn ComputerControl> =
            Arc::new(crate::agents::computer::control::LocalControl::new(executor.clone(), false));
        (MouseTool::new(executor, control, vision, false), calls)
    }

    fn recorded(calls: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
        calls.lock().unwrap().clone()
    }

    #[test]
    fn click_defaults_to_a_single_left_click() {
        let (t, calls) = tool();
        t.execute_with(
            MouseBackend::At,
            "click",
            &json!({"goal": "open", "x": 10, "y": 20}),
        )
        .unwrap();
        let log = recorded(&calls);
        assert!(log.contains(&"move_to(10,20)".to_string()), "{log:?}");
        assert!(log.contains(&"click".to_string()), "{log:?}");
    }

    #[test]
    fn click_forwards_button_and_clicks() {
        let (t, calls) = tool();
        t.execute_with(
            MouseBackend::At,
            "click",
            &json!({"goal": "new tab", "x": 10, "y": 20, "button": "middle"}),
        )
        .unwrap();
        let log = recorded(&calls);
        assert!(log.contains(&"mouse_down(middle)".to_string()), "{log:?}");
        assert!(log.contains(&"mouse_up(middle)".to_string()), "{log:?}");

        let (t, calls) = tool();
        t.execute_with(
            MouseBackend::At,
            "click",
            &json!({"goal": "open", "x": 10, "y": 20, "clicks": 2}),
        )
        .unwrap();
        assert!(recorded(&calls).contains(&"double_click".to_string()));

        let (t, calls) = tool();
        t.execute_with(
            MouseBackend::At,
            "click",
            &json!({"goal": "menu", "x": 10, "y": 20, "button": "right"}),
        )
        .unwrap();
        assert!(recorded(&calls).contains(&"right_click".to_string()));
    }

    #[test]
    fn click_rejects_unknown_button_and_extra_clicks() {
        let (t, _) = tool();
        let err = t
            .execute_with(
                MouseBackend::At,
                "click",
                &json!({"goal": "g", "x": 1, "y": 2, "button": "scroll"}),
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("left, middle, or right"), "{err}");

        let (t, _) = tool();
        assert!(t
            .execute_with(
                MouseBackend::At,
                "click",
                &json!({"goal": "g", "x": 1, "y": 2, "clicks": 3}),
            )
            .is_err());
    }

    #[test]
    fn mouse_down_and_mouse_up_default_to_left() {
        let (t, calls) = tool();
        t.execute_with(MouseBackend::At, "mouse_down", &json!({"goal": "drag"}))
            .unwrap();
        t.execute_with(
            MouseBackend::At,
            "mouse_up",
            &json!({"goal": "drop", "button": "right"}),
        )
        .unwrap();
        let log = recorded(&calls);
        assert!(log.contains(&"mouse_down(left)".to_string()), "{log:?}");
        assert!(log.contains(&"mouse_up(right)".to_string()), "{log:?}");
    }

    #[test]
    fn drag_to_starts_from_the_current_pointer() {
        let (t, calls) = tool();
        t.execute_with(
            MouseBackend::At,
            "hover",
            &json!({"goal": "position", "x": 10, "y": 20}),
        )
        .unwrap();
        t.execute_with(
            MouseBackend::At,
            "drag_to",
            &json!({"goal": "move item", "x": 300, "y": 400}),
        )
        .unwrap();
        let log = recorded(&calls);
        assert!(log.contains(&"move_to(10,20)".to_string()), "{log:?}");
        assert!(log.contains(&"mouse_down(left)".to_string()), "{log:?}");
        assert!(log.contains(&"move_to(300,400)".to_string()), "{log:?}");
        assert!(log.contains(&"mouse_up(left)".to_string()), "{log:?}");
    }

    #[test]
    fn drag_to_rejects_a_zero_length_drag() {
        let (t, _) = tool();
        assert!(t
            .execute_with(
                MouseBackend::At,
                "drag_to",
                &json!({"goal": "nothing", "x": 0, "y": 0}),
            )
            .is_err());
    }

    #[test]
    fn scroll_current_takes_pixels_on_both_axes() {
        let backend = RecordingBackend::default();
        let calls = backend.calls.clone();
        let executor = Arc::new(Mutex::new(ActionExecutor::new(Box::new(backend))));
        scroll_current(&executor, &json!({"goal": "page", "dx": 200, "dy": -300})).unwrap();
        let log = recorded(&calls);
        assert!(log.contains(&"scroll(-3)".to_string()), "{log:?}");
        assert!(log.contains(&"scroll_horizontal(2)".to_string()), "{log:?}");
    }

    #[test]
    fn scroll_current_rejects_a_noop_or_missing_delta() {
        let backend = RecordingBackend::default();
        let executor = Arc::new(Mutex::new(ActionExecutor::new(Box::new(backend))));
        assert!(scroll_current(&executor, &json!({"goal": "page"})).is_err());
        assert!(scroll_current(&executor, &json!({"goal": "page", "dx": 0, "dy": 0})).is_err());
    }
}
