//! Keyboard action tools: `key_press` / `key_down` / `key_up` / `keyboard_type`.
//!
//! These map one-for-one onto OSWorld's `PRESS`, `KEY_DOWN`, `KEY_UP`, and
//! `TYPING` actions (design §4.1 / §7.4). They act on the **currently focused**
//! widget — none of them clicks or moves the pointer.

use super::args_util::{require_non_empty_str, text_from_args};
use crate::agents::computer::actions::{ActionExecutor, KeyPhase};
use crate::agents::computer::verify::VerifyHintGenerator;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// Keyboard action tool (`PRESS` / `KEY_DOWN` / `KEY_UP` / `TYPING`).
pub struct KeyboardTool {
    executor: Arc<Mutex<ActionExecutor>>,
    verify: VerifyHintGenerator,
}

impl KeyboardTool {
    pub fn new(executor: Arc<Mutex<ActionExecutor>>) -> Self {
        Self {
            executor,
            verify: VerifyHintGenerator::new(),
        }
    }

    pub fn execute(&self, method: &str, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        match method {
            "key_press" => self.press(args),
            "key_down" => self.down(args),
            "key_up" => self.up(args),
            "keyboard_type" => self.typing(args),
            _ => Err(anyhow!(
                "Unknown keyboard method: {method}. Use key_press, key_down, key_up, keyboard_type."
            )),
        }
    }

    fn key_arg(args: &Value) -> Result<String> {
        require_non_empty_str(args, "key")
    }

    /// `PRESS` — press and release one key.
    fn press(&self, args: &Value) -> Result<String> {
        let key = Self::key_arg(args)?;
        self.executor.lock().unwrap().press_key(&key)?;
        Ok(self.verify.hotkey_hint(&[key.as_str()]))
    }

    /// `KEY_DOWN` — hold one key until a later `key_up`.
    fn down(&self, args: &Value) -> Result<String> {
        let key = Self::key_arg(args)?;
        self.executor
            .lock()
            .unwrap()
            .key_phase(&key, KeyPhase::Press)?;
        Ok(format!(
            "Held {key} down. It stays held until key_up — release it on a later turn."
        ))
    }

    /// `KEY_UP` — release one key.
    fn up(&self, args: &Value) -> Result<String> {
        let key = Self::key_arg(args)?;
        self.executor
            .lock()
            .unwrap()
            .key_phase(&key, KeyPhase::Release)?;
        Ok(format!("Released {key}."))
    }

    /// `TYPING` — type text into the focused widget (no click, no clipboard).
    fn typing(&self, args: &Value) -> Result<String> {
        let text = text_from_args(args.get("text"))?;
        if text.is_empty() {
            return Err(anyhow!("text cannot be empty"));
        }
        self.executor.lock().unwrap().type_text(&text)?;
        Ok(self.verify.type_hint(&text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::computer::actions::{ActionBackend, ActionResult, MouseButton};
    use crate::agents::computer::input::mouse_move::MouseMoveProfile;
    use serde_json::json;

    #[derive(Default)]
    struct RecordingBackend {
        calls: Arc<Mutex<Vec<String>>>,
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
            Ok(ActionResult::success("move_to"))
        }
        fn scroll(&self, lines: i32) -> Result<ActionResult> {
            self.record(format!("scroll({lines})"));
            Ok(ActionResult::success("scroll"))
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
            Ok((0, 0))
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
            self.record(format!("mouse({},{phase:?})", button.as_str()));
            Ok(ActionResult::success("mouse_phase"))
        }
    }

    fn tool() -> (KeyboardTool, Arc<Mutex<Vec<String>>>) {
        let backend = RecordingBackend::default();
        let calls = backend.calls.clone();
        let executor = Arc::new(Mutex::new(ActionExecutor::new(Box::new(backend))));
        (KeyboardTool::new(executor), calls)
    }

    fn recorded(calls: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
        calls.lock().unwrap().clone()
    }

    #[test]
    fn key_press_is_a_press_release_pair() {
        let (t, calls) = tool();
        t.execute("key_press", &json!({"goal": "submit", "key": "enter"}))
            .unwrap();
        let log = recorded(&calls);
        assert!(log.contains(&"key_down(enter)".to_string()), "{log:?}");
        assert!(log.contains(&"key_up(enter)".to_string()), "{log:?}");
    }

    #[test]
    fn key_down_and_key_up_hold_and_release() {
        let (t, calls) = tool();
        t.execute("key_down", &json!({"goal": "select", "key": "shift"}))
            .unwrap();
        t.execute("key_up", &json!({"goal": "stop", "key": "shift"}))
            .unwrap();
        assert_eq!(
            recorded(&calls),
            vec!["key_down(shift)".to_string(), "key_up(shift)".to_string()]
        );
    }

    #[test]
    fn keyboard_type_types_without_clicking() {
        let (t, calls) = tool();
        t.execute(
            "keyboard_type",
            &json!({"goal": "type", "text": "hello world"}),
        )
        .unwrap();
        let log = recorded(&calls);
        assert_eq!(log, vec!["type_text(hello world)".to_string()]);
    }

    #[test]
    fn missing_key_or_text_is_rejected() {
        let (t, calls) = tool();
        assert!(t.execute("key_press", &json!({"goal": "g"})).is_err());
        assert!(t
            .execute("keyboard_type", &json!({"goal": "g", "text": ""}))
            .is_err());
        assert!(t.execute("unknown", &json!({"goal": "g"})).is_err());
        assert!(recorded(&calls).is_empty(), "no input on a rejected call");
    }
}
