use super::actions::{ActionBackend, ActionResult, KeyPhase, MouseButton};
use super::mouse_move::{execute_move_plan, MouseMovePlanner, MouseMoveProfile};
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

    fn move_to_with_profile(
        &self,
        x: i32,
        y: i32,
        profile: MouseMoveProfile,
    ) -> Result<ActionResult> {
        let mut enigo = self.enigo.borrow_mut();
        let from = enigo
            .location()
            .map_err(|e| anyhow!("Get current position before move failed: {:?}", e))?;
        let target = (x, y);
        if from == target && !profile.exec.pre_jitter {
            return Ok(ActionResult::success(format!(
                "Move skipped (already at ({}, {}))",
                x, y
            )));
        }

        let mut rng = rand::thread_rng();
        let planner = MouseMovePlanner::from_profile(profile);
        let exec_cfg = profile.exec;
        let plan = planner.plan(from, target, &mut rng);

        let move_result = execute_move_plan(
            &plan,
            from,
            target,
            &exec_cfg,
            |px, py| {
                enigo
                    .move_mouse(px, py, enigo::Coordinate::Abs)
                    .map_err(|e| anyhow!("Move failed at ({px}, {py}): {:?}", e))
            },
            &mut rng,
        );

        move_result?;

        let total_secs: f64 = plan.timing.step_intervals_secs.iter().sum();
        if plan.path.points.is_empty() && !profile.exec.pre_jitter {
            Ok(ActionResult::success(format!(
                "Moved to ({}, {}) (direct)",
                x, y
            )))
        } else {
            Ok(ActionResult::success(format!(
                "Moved to ({}, {}) with {} points over {:.2}s",
                x,
                y,
                plan.path.points.len(),
                total_secs
            )))
        }
    }

    fn scroll(&self, lines: i32) -> Result<ActionResult> {
        let mut enigo = self.enigo.borrow_mut();
        // Pointer/Python: positive lines = scroll viewport up; enigo vertical: positive = scroll down.
        enigo
            .scroll(-lines, enigo::Axis::Vertical)
            .map_err(|e| anyhow!("Scroll failed: {:?}", e))?;
        Ok(ActionResult::success(format!(
            "Scrolled {lines} lines (Pointer sign)"
        )))
    }

    fn scroll_horizontal(&self, lines: i32) -> Result<ActionResult> {
        let mut enigo = self.enigo.borrow_mut();
        // Pointer/pyautogui: positive lines = scroll right; enigo mirrors the
        // vertical sign convention (positive = down/right).
        enigo
            .scroll(lines, enigo::Axis::Horizontal)
            .map_err(|e| anyhow!("Horizontal scroll failed: {:?}", e))?;
        Ok(ActionResult::success(format!(
            "Scrolled {lines} lines horizontally"
        )))
    }

    fn type_text(&self, text: &str) -> Result<ActionResult> {
        let mut enigo = self.enigo.borrow_mut();
        enigo
            .text(text)
            .map_err(|e| anyhow!("Type text failed: {:?}", e))?;
        Ok(ActionResult::success(format!("Typed: {}", text)))
    }

    fn type_text_via_clipboard(&self, text: &str) -> Result<ActionResult> {
        // 1+2. Save the old clipboard content and write the target text in a
        // single main-thread hop — NSPasteboard may only be touched there.
        //
        // No settle delay: `replace_text` returns only after `NSPasteboard
        // setString:forType:` has completed, and the pasteboard is readable
        // immediately afterwards (`pbcopy` then `pbpaste` in the next process
        // reads the new value back). A sleep here bought nothing.
        let saved = crate::platform::clipboard::replace_text(text)?;

        // 3. Paste via Cmd+V / Ctrl+V (synthetic key events need no main thread).
        let modifier = if cfg!(target_os = "macos") {
            "command"
        } else {
            "ctrl"
        };
        // All four key events go through a single main-thread hop. enigo's macOS
        // backend resolves `Key::Unicode` through Carbon Text Input Source APIs
        // (`TISCopyCurrentKeyboardInputSource` / `TISGetInputSourceProperty`),
        // which carry an internal `dispatch_assert_queue(main)`. Calling them
        // from a Tokio worker aborts the process — a native abort that
        // `catch_unwind` cannot intercept and that logs nothing.
        crate::platform::run_synthetic_input(|| -> Result<()> {
            let mut enigo = self.enigo.borrow_mut();
            enigo
                .key(parse_key_name(modifier)?, Press)
                .map_err(|e| anyhow!("Paste hotkey press failed: {e}"))?;
            enigo
                .key(Key::Unicode('v'), Press)
                .map_err(|e| anyhow!("Paste key 'v' press failed: {e}"))?;
            // No inter-key delay: measured against TextEdit with 0 / 5 / 10 / 20 ms
            // between key-down and key-up, all four gaps pasted 30/30.
            enigo
                .key(Key::Unicode('v'), Release)
                .map_err(|e| anyhow!("Paste key 'v' release failed: {e}"))?;
            enigo
                .key(parse_key_name(modifier)?, Release)
                .map_err(|e| anyhow!("Paste hotkey release failed: {e}"))?;
            Ok(())
        })?;
        // Wait before restoring the clipboard: the target app reads the
        // pasteboard asynchronously, and nothing reports when it is done.
        // Measured on TextEdit — 0/5/10 ms pasted the *old* clipboard text
        // every time, 30 ms still lost 10/30, 50 ms was 30/30. The value is a
        // setting because slow targets (Electron editors, remote desktops) may
        // need longer than the measured floor.
        let restore_delay_ms =
            crate::platform_config::effective_settings_global().computer_paste_restore_delay_ms;
        std::thread::sleep(Duration::from_millis(u64::from(restore_delay_ms)));

        // 4. Restore old clipboard
        if let Some(prev) = saved {
            let _ = crate::platform::clipboard::write_text(&prev);
        }

        Ok(ActionResult::success(format!(
            "Pasted via clipboard: {}",
            text
        )))
    }

    fn hotkey(&self, keys: &[&str]) -> Result<ActionResult> {
        // Same reason as `type_text_via_clipboard`: any single-character key
        // name resolves to `Key::Unicode`, which reaches Carbon Text Input
        // Source APIs that assert main-thread execution.
        crate::platform::run_synthetic_input(|| -> Result<()> {
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
            Ok(())
        })?;

        Ok(ActionResult::success(format!(
            "Pressed hotkey: {}",
            keys.join("+")
        )))
    }

    fn get_position(&self) -> Result<(i32, i32)> {
        let enigo = self.enigo.borrow();
        let (x, y) = enigo
            .location()
            .map_err(|e| anyhow!("Get position failed: {:?}", e))?;
        Ok((x, y))
    }

    fn key_phase(&self, name: &str, phase: KeyPhase) -> Result<ActionResult> {
        let dir = match phase {
            KeyPhase::Press => Press,
            KeyPhase::Release => Release,
        };
        // Same main-thread requirement as `hotkey`: `parse_key_name` yields
        // `Key::Unicode` for single characters, and enigo resolves that through
        // Carbon Text Input Source APIs that assert main-thread execution.
        crate::platform::run_synthetic_input(|| -> Result<()> {
            let mut enigo = self.enigo.borrow_mut();
            let key = parse_key_name(name)?;
            enigo
                .key(key, dir)
                .map_err(|e| anyhow!("Key {:?} failed: {:?}", phase, e))?;
            Ok(())
        })?;
        Ok(ActionResult::success(format!("key {name} {phase:?}")))
    }

    fn mouse_phase(&self, button: MouseButton, phase: KeyPhase) -> Result<ActionResult> {
        let mut enigo = self.enigo.borrow_mut();
        let b = match button {
            MouseButton::Left => enigo::Button::Left,
            MouseButton::Middle => enigo::Button::Middle,
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
/// - Modifiers: "command"/"cmd" (macOS Command), "win"/"meta"/"super" (OS meta key),
///   "control"/"ctrl", "alt"/"option", "shift"
/// - Special keys: "return", "enter", "tab", "space", "escape", "backspace", "delete"
/// - Arrow keys: "up", "down", "left", "right"
/// - Letters and numbers are passed through directly.
fn parse_key_name(name: &str) -> Result<Key> {
    let lower = name.to_lowercase();
    match lower.as_str() {
        "command" | "cmd" => {
            #[cfg(target_os = "macos")]
            return Ok(Key::Meta);
            #[cfg(not(target_os = "macos"))]
            return Ok(Key::Control);
        }
        "meta" | "win" | "windows" | "super" | "lwin" | "rwin" => Ok(Key::Meta),
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
        assert_eq!(parse_key_name("win").unwrap(), Key::Meta);
        assert_eq!(parse_key_name("meta").unwrap(), Key::Meta);
        assert_eq!(parse_key_name("super").unwrap(), Key::Meta);
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
