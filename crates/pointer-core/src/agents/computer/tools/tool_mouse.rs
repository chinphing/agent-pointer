use super::args_util::{
    click_count_from_args, mouse_button_from_args, require_non_empty_str, required_f32_arg,
    scroll_notches_from_args, ScrollNotches, SCROLL_NOTCHES_MAX,
};
use super::method_route::MouseBackend;
use crate::agents::computer::control::ComputerControl;
use crate::agents::computer::verify::VerifyHintGenerator;
use crate::agents::computer::vision_state::VisionState;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::{Arc, Mutex};

// ── MouseAtTool ──────────────────────────────────────────────────────────────

/// Mouse actions using logical screen pixel coordinates.
pub struct MouseAtTool {
    /// Every action goes through the L1 abstraction, so the same code path runs
    /// locally and over a remote control channel. `human_like` is a property of
    /// the control implementation, not a per-call override.
    control: Arc<dyn ComputerControl>,
    vision_state: Arc<Mutex<VisionState>>,
    verify: VerifyHintGenerator,
}

impl MouseAtTool {
    pub fn new(control: Arc<dyn ComputerControl>, vision_state: Arc<Mutex<VisionState>>) -> Self {
        Self {
            control,
            vision_state,
            verify: VerifyHintGenerator::new(),
        }
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
            "scroll_current" => self.scroll_current(args),
            _ => Err(anyhow!(
                "Unknown mouse_at method: {method}. Use click, double_click, right_click, hover, drag_from_to, mouse_down, mouse_up, scroll_current."
            )),
        }
    }

    /// Scroll by wheel notches, optionally moving the pointer onto the panel first.
    ///
    /// Wheel events land on whatever window sits under the pointer, so
    /// "scroll that list" needs a pointer move before the scroll. `x`/`y` are
    /// optional: without them this is the at-current-pointer behaviour.
    fn scroll_current(&self, args: &Value) -> Result<String> {
        let n = scroll_notches_from_args(args)?;
        let moved = match (args.get("x"), args.get("y")) {
            (Some(_), Some(_)) => {
                let x = required_f32_arg(args, "x")?;
                let y = required_f32_arg(args, "y")?;
                let (px, py) = self.resolve_coord(args, x, y)?;
                self.control.move_to(px, py)?;
                Some((px, py))
            }
            _ => None,
        };
        self.control.scroll(n.x, n.y)?;
        Ok(scroll_report(n, moved))
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
        if px1 == px2 && py1 == py2 {
            return Err(anyhow!("drag start and end must differ"));
        }
        // Park the pointer on the grab point, then hand off to the trait's
        // `drag_to` — the OSWorld `DRAG_TO` primitive (press at the current
        // pointer, move, release). The move above is what makes the grab point
        // deterministic; doing the down/move/up sequence by hand here would
        // duplicate `drag_left_from_to` and drift from it over time.
        //
        // `move_to` is idempotent on the backend (enigo skips the move when the
        // pointer is already there), so the `drag_to` that follows re-entering
        // the same coordinate costs nothing.
        self.control.move_to(px1, py1)?;
        self.control.drag_to(px2, py2)?;
        Ok(format!(
            "Dragged ({}, {}) -> ({}, {}). Verify on next screenshot.",
            px1, py1, px2, py2
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

/// `mouse_scroll_current` — scroll at the current pointer by **wheel notches**.
///
/// Positive `notches_y` scrolls up, positive `notches_x` scrolls right. Notches
/// reach the OS wheel primitive unchanged: one notch is whatever the platform
/// and the app decide, so there is no pixel distance to report or promise.
pub fn scroll_current(control: &Arc<dyn ComputerControl>, args: &Value) -> Result<String> {
    require_non_empty_str(args, "goal")?;
    let n = scroll_notches_from_args(args)?;
    control.scroll(n.x, n.y)?;
    Ok(scroll_report(n, None))
}

/// Build the tool result line, including the clamp notice when the request
/// exceeded ±[`SCROLL_NOTCHES_MAX`]. Without that notice the model keeps
/// believing its `-900` was honoured.
fn scroll_report(n: ScrollNotches, moved: Option<(i32, i32)>) -> String {
    let mut s = match moved {
        Some((px, py)) => format!(
            "Grounded the panel at ({px}, {py}) — the locator re-finds it from \
             target_desc, so this may differ from the bbox you passed — then \
             scrolled {}×{} notches there.",
            n.x, n.y
        ),
        None => format!("Scrolled {}×{} notches at the current pointer.", n.x, n.y),
    };
    if let Some((raw_x, raw_y)) = n.clamped_from {
        s.push_str(&format!(
            " Requested {raw_x}×{raw_y}, clamped to ±{SCROLL_NOTCHES_MAX} notches."
        ));
    }
    s
}

// ── MouseTool (unified registry entry) ───────────────────────────────────────

/// Unified mouse tool: routes `click_at` / `click_index` etc. by method name.
pub struct MouseTool {
    at: MouseAtTool,
}

impl MouseTool {
    pub fn new(control: Arc<dyn ComputerControl>, vision_state: Arc<Mutex<VisionState>>) -> Self {
        Self {
            at: MouseAtTool::new(control, vision_state),
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
    use crate::agents::computer::actions::ActionExecutor;
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
        (MouseTool::new(control, vision), calls)
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
    fn drag_from_to_parks_on_the_grab_point_then_defers_to_drag_to() {
        // 起点由参数给出：先 move_to 把指针停到抓取点，再交给 drag_to
        // （它按当前位置按下）。不再自己拼 down/move/up，那条序列在
        // `drag_left_from_to` 里已经有了，两份实现迟早会漂移。
        let (t, calls) = tool();
        t.execute_with(
            MouseBackend::At,
            "drag_from_to",
            &json!({"goal": "file to trash", "x1": 10, "y1": 20, "x2": 300, "y2": 400}),
        )
        .unwrap();
        let log = recorded(&calls);
        // 抓取点必须先被设定，否则 drag_to 会从上一个残留位置开始拖
        assert_eq!(log[0], "move_to(10,20)", "{log:?}");
        assert!(log.contains(&"mouse_down(left)".to_string()), "{log:?}");
        assert!(log.contains(&"move_to(300,400)".to_string()), "{log:?}");
        assert!(log.contains(&"mouse_up(left)".to_string()), "{log:?}");
        // 按下必��发生在抓取点之后、拖到终点之前
        let down = log.iter().position(|c| c == "mouse_down(left)").unwrap();
        let to_end = log.iter().position(|c| c == "move_to(300,400)").unwrap();
        let up = log.iter().position(|c| c == "mouse_up(left)").unwrap();
        assert!(down < to_end && to_end < up, "{log:?}");
    }

    #[test]
    fn drag_from_to_rejects_identical_endpoints() {
        // grounding 若把两端解析到同一点，发出去就是「原地按下松开」的空动作
        let (t, calls) = tool();
        assert!(t
            .execute_with(
                MouseBackend::At,
                "drag_from_to",
                &json!({"goal": "nothing", "x1": 50, "y1": 60, "x2": 50, "y2": 60}),
            )
            .is_err());
        assert!(recorded(&calls).is_empty(), "{:?}", recorded(&calls));
    }

    fn scroll_control() -> (Arc<dyn ComputerControl>, Arc<Mutex<Vec<String>>>) {
        let backend = RecordingBackend::default();
        let calls = backend.calls.clone();
        let executor = Arc::new(Mutex::new(ActionExecutor::new(Box::new(backend))));
        let control: Arc<dyn ComputerControl> =
            Arc::new(crate::agents::computer::control::LocalControl::new(executor, false));
        (control, calls)
    }

    #[test]
    fn scroll_current_passes_notches_on_both_axes() {
        let (control, calls) = scroll_control();
        scroll_current(
            &control,
            &json!({"goal": "page", "notches_x": 20, "notches_y": -30}),
        )
        .unwrap();
        let log = recorded(&calls);
        assert!(log.contains(&"scroll(-30)".to_string()), "{log:?}");
        assert!(log.contains(&"scroll_horizontal(20)".to_string()), "{log:?}");
    }

    #[test]
    fn scroll_current_rejects_a_noop_or_missing_delta() {
        let (control, _calls) = scroll_control();
        assert!(scroll_current(&control, &json!({"goal": "page"})).is_err());
        assert!(scroll_current(
            &control,
            &json!({"goal": "page", "notches_x": 0, "notches_y": 0})
        )
        .is_err());
    }

    #[test]
    fn scroll_current_reports_the_clamp_instead_of_failing() {
        let (control, calls) = scroll_control();
        // -900 was the pixel-era value. It must not fail — but the result has to
        // say it was clamped, or the model keeps believing it scrolled 900.
        let out = scroll_current(&control, &json!({"goal": "page", "notches_y": -900})).unwrap();
        assert!(out.contains("-100"), "{out}");
        assert!(out.contains("-900"), "{out}");
        assert!(out.contains("clamped"), "{out}");
        let log = recorded(&calls);
        assert!(log.contains(&"scroll(-100)".to_string()), "{log:?}");
    }

    #[test]
    fn scroll_current_moves_the_pointer_first_when_given_a_point() {
        // Wheel events reach the window under the pointer, so a grounded target
        // must move the pointer before scrolling. Without x/y the old
        // at-current-pointer behaviour stays.
        let (t, calls) = tool();
        t.execute_with(
            MouseBackend::At,
            "scroll_current",
            &json!({"goal": "page", "notches_y": -50, "x": 640, "y": 400}),
        )
        .unwrap();
        let log = recorded(&calls);
        let move_at = log.iter().position(|c| c.starts_with("move_to("));
        let scroll_at = log.iter().position(|c| c.starts_with("scroll("));
        assert!(move_at.is_some(), "expected a move_to, got {log:?}");
        assert!(scroll_at.is_some(), "expected a scroll, got {log:?}");
        assert!(
            move_at.unwrap() < scroll_at.unwrap(),
            "move must come before scroll, got {log:?}"
        );
    }

    #[test]
    fn scroll_current_without_a_point_does_not_move_the_pointer() {
        let (t, calls) = tool();
        t.execute_with(
            MouseBackend::At,
            "scroll_current",
            &json!({"goal": "page", "notches_y": -50}),
        )
        .unwrap();
        let log = recorded(&calls);
        assert!(
            !log.iter().any(|c| c.starts_with("move_to(")),
            "must not move without x/y, got {log:?}"
        );
        assert!(log.iter().any(|c| c.starts_with("scroll(")), "{log:?}");
    }
}
