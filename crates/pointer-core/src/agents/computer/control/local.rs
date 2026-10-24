//! `LocalControl`: drives this host through the existing [`ActionExecutor`].
//!
//! Behaviour is intentionally identical to the tools' current call paths:
//! pointer moves still honour `human_like`, and the post-action settles inside
//! `ActionExecutor` still run. This type only translates trait calls.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};

use super::ComputerControl;
use crate::agents::computer::input::actions::{ActionExecutor, KeyPhase, MouseButton};
use crate::agents::computer::vision::screen;

/// Local implementation of [`ComputerControl`].
pub struct LocalControl {
    executor: Arc<Mutex<ActionExecutor>>,
    human_like: bool,
}

impl LocalControl {
    /// Build from an existing executor. `human_like` mirrors the setting the
    /// tools pass on every call.
    pub fn new(executor: Arc<Mutex<ActionExecutor>>, human_like: bool) -> Self {
        Self {
            executor,
            human_like,
        }
    }

    fn exec(&self) -> std::sync::MutexGuard<'_, ActionExecutor> {
        self.executor.lock().unwrap()
    }
}

impl ComputerControl for LocalControl {
    fn move_to(&self, x: i32, y: i32) -> Result<()> {
        self.exec().move_to(x, y, self.human_like)?;
        Ok(())
    }

    fn click(
        &self,
        button: MouseButton,
        x: Option<i32>,
        y: Option<i32>,
        clicks: u32,
    ) -> Result<()> {
        let exec = self.exec();
        match (x, y) {
            (Some(x), Some(y)) => {
                exec.click_button_at(button, x, y, clicks, self.human_like)?;
            }
            (None, None) => {
                exec.click_button_here(button, clicks)?;
            }
            _ => return Err(anyhow!("click needs both x and y, or neither")),
        }
        Ok(())
    }

    fn mouse_down(&self, button: MouseButton) -> Result<()> {
        self.exec().mouse_phase(button, KeyPhase::Press)?;
        Ok(())
    }

    fn mouse_up(&self, button: MouseButton) -> Result<()> {
        self.exec().mouse_phase(button, KeyPhase::Release)?;
        Ok(())
    }

    fn release_stranded_input(&self) -> Result<()> {
        let exec = self
            .executor
            .lock()
            .map_err(|e| anyhow!("executor lock poisoned: {e}"))?;
        exec.release_stranded_input()
    }

    fn right_click(&self, x: i32, y: i32) -> Result<()> {
        self.exec().right_click_at(x, y, self.human_like)?;
        Ok(())
    }

    fn double_click(&self, x: i32, y: i32) -> Result<()> {
        self.exec().double_click_at(x, y, self.human_like)?;
        Ok(())
    }

    fn drag_to(&self, x: i32, y: i32) -> Result<()> {
        self.exec().drag_left_from_current_to(x, y, self.human_like)?;
        Ok(())
    }

    /// `SCROLL` — wheel notches, passed straight to the backend.
    ///
    /// Positive `dy` scrolls up, positive `dx` scrolls right (pyautogui sign
    /// convention, which the remote backend forwards verbatim).
    fn scroll(&self, dx: i32, dy: i32) -> Result<()> {
        if dx == 0 && dy == 0 {
            return Err(anyhow!("scroll needs a non-zero dx or dy"));
        }
        self.exec().scroll_at_current_notches(dx, dy)?;
        Ok(())
    }

    fn typing(&self, text: &str) -> Result<()> {
        self.exec().type_text(text)?;
        Ok(())
    }

    fn press(&self, key: &str) -> Result<()> {
        self.exec().press_key(key)?;
        Ok(())
    }

    fn key_down(&self, key: &str) -> Result<()> {
        self.exec().key_phase(key, KeyPhase::Press)?;
        Ok(())
    }

    fn key_up(&self, key: &str) -> Result<()> {
        self.exec().key_phase(key, KeyPhase::Release)?;
        Ok(())
    }

    fn hotkey(&self, keys: &[String]) -> Result<()> {
        let refs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
        self.exec().hotkey(&refs)?;
        Ok(())
    }

    fn wait(&self, ms: u64) -> Result<()> {
        std::thread::sleep(Duration::from_millis(ms));
        Ok(())
    }

    fn screenshot(&self) -> Result<Vec<u8>> {
        Ok(screen::screenshot_current_monitor()?.jpeg)
    }

    fn screen_size(&self) -> Result<(u32, u32)> {
        let monitors = screen::list_monitors()?;
        let primary = monitors
            .iter()
            .find(|m| m.is_primary)
            .or_else(|| monitors.first())
            .ok_or_else(|| anyhow!("no monitors reported"))?;
        Ok((primary.width.max(1) as u32, primary.height.max(1) as u32))
    }

    fn cursor_position(&self) -> Result<(i32, i32)> {
        self.exec().get_position()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::computer::input::actions::{ActionBackend, ActionResult};
    use crate::agents::computer::input::mouse_move::MouseMoveProfile;

    /// Records every backend call so tests can assert the translation.
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
            profile: MouseMoveProfile,
        ) -> Result<ActionResult> {
            self.record(format!("move_to({x},{y})"));
            self.record(format!("profile({:?})", profile.path.strategy));
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
            let b = match button {
                MouseButton::Left => "left",
                MouseButton::Middle => "middle",
                MouseButton::Right => "right",
            };
            let p = match phase {
                KeyPhase::Press => "down",
                KeyPhase::Release => "up",
            };
            self.record(format!("mouse_{p}({b})"));
            Ok(ActionResult::success("mouse_phase"))
        }
    }

    fn control() -> (LocalControl, Arc<Mutex<Vec<String>>>) {
        control_with(false)
    }

    fn control_with(human_like: bool) -> (LocalControl, Arc<Mutex<Vec<String>>>) {
        let backend = RecordingBackend::default();
        let calls = backend.calls.clone();
        let executor = Arc::new(Mutex::new(ActionExecutor::new(Box::new(backend))));
        (LocalControl::new(executor, human_like), calls)
    }

    fn recorded(calls: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
        calls.lock().unwrap().clone()
    }

    #[test]
    fn click_with_coordinates_picks_the_matching_helper() {
        let (c, calls) = control();
        c.click(MouseButton::Left, Some(10), Some(20), 1).unwrap();
        assert!(recorded(&calls).iter().any(|c| c == "move_to(10,20)"));

        let (c, calls) = control();
        c.click(MouseButton::Left, Some(10), Some(20), 2).unwrap();
        assert!(recorded(&calls).contains(&"double_click".to_string()));

        let (c, calls) = control();
        c.click(MouseButton::Right, Some(10), Some(20), 1).unwrap();
        assert!(recorded(&calls).contains(&"right_click".to_string()));
    }

    #[test]
    fn click_without_coordinates_does_not_move() {
        let (c, calls) = control();
        c.click(MouseButton::Left, None, None, 1).unwrap();
        let log = recorded(&calls);
        assert!(log.contains(&"click".to_string()));
        assert!(!log.iter().any(|c| c.starts_with("move_to")));
    }

    #[test]
    fn click_rejects_unsupported_counts_and_half_points() {
        let (c, _) = control();
        assert!(c.click(MouseButton::Left, Some(1), Some(2), 3).is_err());
        assert!(c.click(MouseButton::Right, Some(1), Some(2), 2).is_err());
        assert!(c.click(MouseButton::Left, Some(1), None, 1).is_err());
    }

    #[test]
    fn mouse_phase_maps_to_down_and_up() {
        let (c, calls) = control();
        c.mouse_down(MouseButton::Left).unwrap();
        c.mouse_up(MouseButton::Right).unwrap();
        let log = recorded(&calls);
        assert!(log.contains(&"mouse_down(left)".to_string()));
        assert!(log.contains(&"mouse_up(right)".to_string()));
    }

    #[test]
    fn key_actions_map_to_press_release_pairs() {
        let (c, calls) = control();
        c.press("enter").unwrap();
        c.key_down("shift").unwrap();
        c.key_up("shift").unwrap();
        let log = recorded(&calls);
        assert!(log.contains(&"key_down(enter)".to_string()));
        assert!(log.contains(&"key_up(enter)".to_string()));
        assert!(log.contains(&"key_down(shift)".to_string()));
        assert!(log.contains(&"key_up(shift)".to_string()));
    }

    #[test]
    fn hotkey_forwards_every_key_in_order() {
        let (c, calls) = control();
        c.hotkey(&["command".to_string(), "c".to_string()]).unwrap();
        assert!(recorded(&calls).contains(&"hotkey(command+c)".to_string()));
    }

    #[test]
    fn drag_to_starts_from_the_current_pointer() {
        let (c, calls) = control();
        c.move_to(100, 200).unwrap();
        c.drag_to(300, 400).unwrap();
        let log = recorded(&calls);
        assert!(log.contains(&"move_to(100,200)".to_string()));
        assert!(log.contains(&"mouse_down(left)".to_string()));
        assert!(log.contains(&"move_to(300,400)".to_string()));
        assert!(log.contains(&"mouse_up(left)".to_string()));
    }

    #[test]
    fn scroll_passes_notches_through_on_both_axes() {
        let (c, calls) = control();
        // Notches are the wire format — no conversion happens here.
        c.scroll(20, 30).unwrap();
        let log = recorded(&calls);
        assert!(log.contains(&"scroll(30)".to_string()), "{log:?}");
        assert!(log.contains(&"scroll_horizontal(20)".to_string()), "{log:?}");

        let (c, calls) = control();
        c.scroll(0, -30).unwrap();
        let log = recorded(&calls);
        assert!(log.contains(&"scroll(-30)".to_string()), "{log:?}");
        assert!(!log.iter().any(|c| c.starts_with("scroll_horizontal")));

        let (c, _) = control();
        assert!(c.scroll(0, 0).is_err());
    }

    #[test]
    fn click_supports_the_middle_button() {
        let (c, calls) = control();
        c.click(MouseButton::Middle, Some(7), Some(8), 1).unwrap();
        let log = recorded(&calls);
        assert!(log.contains(&"move_to(7,8)".to_string()), "{log:?}");
        assert!(log.contains(&"mouse_down(middle)".to_string()), "{log:?}");
        assert!(log.contains(&"mouse_up(middle)".to_string()), "{log:?}");
        assert!(c.click(MouseButton::Middle, Some(7), Some(8), 2).is_err());
    }

    #[test]
    fn typing_forwards_text() {
        let (c, calls) = control();
        c.typing("hello").unwrap();
        assert!(recorded(&calls).contains(&"type_text(hello)".to_string()));
    }

    #[test]
    fn cursor_position_reports_backend_position() {
        let (c, _) = control();
        c.move_to(42, 24).unwrap();
        assert_eq!(c.cursor_position().unwrap(), (42, 24));
    }
    /// `human_like` is the only thing that decides the move profile. When it is
    /// on, the backend receives a Bézier path; when off, a straight line. This
    /// is what makes the mouse actually curve instead of sliding in a line.
    #[test]
    fn human_like_selects_the_bezier_profile() {
        let (c, calls) = control_with(true);
        c.move_to(10, 20).unwrap();
        let recorded = recorded(&calls);
        assert!(
            recorded.iter().any(|c| c.contains("Bezier")),
            "human_like should plan a Bezier path, got: {recorded:?}"
        );
    }

    #[test]
    fn standard_selects_the_linear_profile() {
        let (c, calls) = control_with(false);
        c.move_to(10, 20).unwrap();
        let recorded = recorded(&calls);
        assert!(
            recorded.iter().any(|c| c.contains("LinearByPointCount")),
            "human_like off should plan a straight line, got: {recorded:?}"
        );
    }

}

impl Drop for LocalControl {
    fn drop(&mut self) {
        // Best effort: a dropped control must never leave keys or buttons held.
        if let Ok(exec) = self.executor.lock() {
            let _ = exec.release_stranded_input();
        }
    }
}
