use super::actions::{ActionBackend, ActionResult, KeyPhase, MouseButton};
use super::mouse_move::{execute_move_plan, MouseMoveConfig, MouseMovePlanner};
use super::timing::DOUBLE_CLICK_INTERVAL_MS;
use anyhow::{anyhow, Result};
use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Mouse, Settings,
};
use std::cell::RefCell;
use std::time::Duration;

/// enigo-based implementation of ActionBackend.
///
/// Provides cross-platform input simulation using the enigo library.
/// On macOS, this requires Accessibility permissions.
///
/// # Thread Safety
/// Enigo is not thread-safe on macOS. This backend uses RefCell and requires
/// single-threaded access. The ActionExecutor wrapper handles synchronization.
pub struct EnigoBackend {
    enigo: RefCell<Enigo>,
}

unsafe impl Send for EnigoBackend {}
unsafe impl Sync for EnigoBackend {}

impl EnigoBackend {
    /// Create a new EnigoBackend.
    ///
    /// # Errors
    /// Returns an error if the enigo instance cannot be created.
    pub fn new() -> Result<Self> {
        let settings = Settings::default();
        let enigo = Enigo::new(&settings)
            .map_err(|e| anyhow!("Failed to create enigo instance: {:?}", e))?;
        Ok(Self {
            enigo: RefCell::new(enigo),
        })
    }
}

impl ActionBackend for EnigoBackend {
    fn click(&self) -> Result<ActionResult> {
        let mut enigo = self.enigo.borrow_mut();
        enigo
            .button(enigo::Button::Left, Click)
            .map_err(|e| anyhow!("Click failed: {:?}", e))?;
        Ok(ActionResult::success("Clicked"))
    }

    fn double_click(&self) -> Result<ActionResult> {
        let mut enigo = self.enigo.borrow_mut();
        enigo
            .button(enigo::Button::Left, Click)
            .map_err(|e| anyhow!("Double click failed: {:?}", e))?;
        std::thread::sleep(Duration::from_millis(DOUBLE_CLICK_INTERVAL_MS));
        enigo
            .button(enigo::Button::Left, Click)
            .map_err(|e| anyhow!("Double click failed: {:?}", e))?;
        Ok(ActionResult::success("Double-clicked"))
    }

    fn right_click(&self) -> Result<ActionResult> {
        let mut enigo = self.enigo.borrow_mut();
        enigo
            .button(enigo::Button::Right, Click)
            .map_err(|e| anyhow!("Right click failed: {:?}", e))?;
        Ok(ActionResult::success("Right-clicked"))
    }

    fn move_to(&self, x: i32, y: i32) -> Result<ActionResult> {
        let mut enigo = self.enigo.borrow_mut();
        let from = enigo
            .location()
            .map_err(|e| anyhow!("Get current position before move failed: {:?}", e))?;
        let planner = MouseMovePlanner::new(MouseMoveConfig::default());
        let plan = planner.plan(from, (x, y));
        execute_move_plan(&plan, (x, y), |px, py| {
            enigo
                .move_mouse(px, py, enigo::Coordinate::Abs)
                .map_err(|e| anyhow!("Move failed at ({px}, {py}): {:?}", e))
        })?;
        if plan.path.points.is_empty() {
            Ok(ActionResult::success(format!(
                "Move skipped (already at ({}, {}))",
                x, y
            )))
        } else {
            Ok(ActionResult::success(format!(
                "Moved to ({}, {}) with {} points over {:.2}s (eased)",
                x,
                y,
                plan.path.points.len(),
                plan.timing.step_intervals_secs.iter().sum::<f64>()
            )))
        }
    }

    fn scroll(&self, lines: i32) -> Result<ActionResult> {
        let mut enigo = self.enigo.borrow_mut();
        // Pointer/Python: positive lines = scroll viewport up; enigo vertical: positive = scroll down.
        enigo
            .scroll(-lines, enigo::Axis::Vertical)
            .map_err(|e| anyhow!("Scroll failed: {:?}", e))?;
        Ok(ActionResult::success(format!("Scrolled {lines} lines (Pointer sign)")))
    }

    fn type_text(&self, text: &str) -> Result<ActionResult> {
        let mut enigo = self.enigo.borrow_mut();
        enigo
            .text(text)
            .map_err(|e| anyhow!("Type text failed: {:?}", e))?;
        Ok(ActionResult::success(format!("Typed: {}", text)))
    }

    fn hotkey(&self, keys: &[&str]) -> Result<ActionResult> {
        let mut enigo = self.enigo.borrow_mut();
        let enigo_keys: Vec<Key> = keys
            .iter()
            .map(|&k| parse_key_name(k))
            .collect::<Result<Vec<_>>>()?;

        for key in &enigo_keys {
            enigo
                .key(key.clone(), Press)
                .map_err(|e| anyhow!("Hotkey press failed: {:?}", e))?;
        }

        for key in enigo_keys.iter().rev() {
            enigo
                .key(key.clone(), Release)
                .map_err(|e| anyhow!("Hotkey release failed: {:?}", e))?;
        }

        Ok(ActionResult::success(format!(
            "Pressed hotkey: {}",
            keys.join("+")
        )))
    }

    fn get_position(&self) -> Result<(i32, i32)> {
        let enigo = self.enigo.borrow();
        let (x, y) = enigo.location().map_err(|e| anyhow!("Get position failed: {:?}", e))?;
        Ok((x, y))
    }

    fn key_phase(&self, name: &str, phase: KeyPhase) -> Result<ActionResult> {
        let mut enigo = self.enigo.borrow_mut();
        let key = parse_key_name(name)?;
        let dir = match phase {
            KeyPhase::Press => Press,
            KeyPhase::Release => Release,
        };
        enigo
            .key(key, dir)
            .map_err(|e| anyhow!("Key {:?} failed: {:?}", phase, e))?;
        Ok(ActionResult::success(format!("key {name} {phase:?}")))
    }

    fn mouse_phase(&self, button: MouseButton, phase: KeyPhase) -> Result<ActionResult> {
        let mut enigo = self.enigo.borrow_mut();
        let b = match button {
            MouseButton::Left => enigo::Button::Left,
            MouseButton::Right => enigo::Button::Right,
        };
        let dir = match phase {
            KeyPhase::Press => Press,
            KeyPhase::Release => Release,
        };
        enigo
            .button(b, dir)
            .map_err(|e| anyhow!("Mouse button {:?} failed: {:?}", phase, e))?;
        Ok(ActionResult::success("mouse phase"))
    }
}

/// Parse a key name string into an enigo Key.
///
/// Supports common key names:
/// - Modifiers: "command"/"cmd", "control"/"ctrl", "alt"/"option", "shift"
/// - Special keys: "return", "enter", "tab", "space", "escape", "backspace", "delete"
/// - Arrow keys: "up", "down", "left", "right"
/// - Letters and numbers are passed through directly.
fn parse_key_name(name: &str) -> Result<Key> {
    let lower = name.to_lowercase();
    match lower.as_str() {
        "command" | "cmd" | "meta" => {
            #[cfg(target_os = "macos")]
            return Ok(Key::Meta);
            #[cfg(not(target_os = "macos"))]
            return Ok(Key::Control);
        }
        "control" | "ctrl" => Ok(Key::Control),
        "alt" | "option" | "opt" => Ok(Key::Alt),
        "shift" => Ok(Key::Shift),
        "return" | "enter" => Ok(Key::Return),
        "tab" => Ok(Key::Tab),
        "space" => Ok(Key::Space),
        "escape" | "esc" => Ok(Key::Escape),
        "backspace" => Ok(Key::Backspace),
        "delete" => Ok(Key::Delete),
        "up" => Ok(Key::UpArrow),
        "down" => Ok(Key::DownArrow),
        "left" => Ok(Key::LeftArrow),
        "right" => Ok(Key::RightArrow),
        "home" => Ok(Key::Home),
        "end" => Ok(Key::End),
        "pageup" => Ok(Key::PageUp),
        "pagedown" => Ok(Key::PageDown),
        _ => {
            if lower.len() == 1 {
                let ch = lower.chars().next().unwrap();
                if ch.is_ascii_alphanumeric() || ch.is_ascii_punctuation() {
                    return Ok(Key::Unicode(ch));
                }
            }
            Err(anyhow!("Unknown key name: {}", name))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_key_name_modifiers() {
        assert_eq!(parse_key_name("ctrl").unwrap(), Key::Control);
        assert_eq!(parse_key_name("alt").unwrap(), Key::Alt);
        assert_eq!(parse_key_name("shift").unwrap(), Key::Shift);
    }

    #[test]
    fn test_parse_key_name_special() {
        assert_eq!(parse_key_name("return").unwrap(), Key::Return);
        assert_eq!(parse_key_name("tab").unwrap(), Key::Tab);
        assert_eq!(parse_key_name("space").unwrap(), Key::Space);
        assert_eq!(parse_key_name("escape").unwrap(), Key::Escape);
    }

    #[test]
    fn test_parse_key_name_arrows() {
        assert_eq!(parse_key_name("up").unwrap(), Key::UpArrow);
        assert_eq!(parse_key_name("down").unwrap(), Key::DownArrow);
        assert_eq!(parse_key_name("left").unwrap(), Key::LeftArrow);
        assert_eq!(parse_key_name("right").unwrap(), Key::RightArrow);
    }

    #[test]
    fn test_parse_key_name_alphanumeric() {
        assert_eq!(parse_key_name("a").unwrap(), Key::Unicode('a'));
        assert_eq!(parse_key_name("1").unwrap(), Key::Unicode('1'));
    }

    #[test]
    fn test_parse_key_name_unknown() {
        assert!(parse_key_name("unknown_key").is_err());
    }
}
