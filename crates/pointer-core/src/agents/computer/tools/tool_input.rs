use super::args_util::{
    json_bool_loose, require_non_empty_str, required_f32_arg, text_from_args,
};
use super::method_route::InputBackend;
use crate::agents::computer::control::ComputerControl;
use crate::agents::computer::input::actions::MouseButton;
use crate::agents::computer::verify::VerifyHintGenerator;
use crate::agents::computer::vision_state::VisionState;
use anyhow::Result;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Gap between the primitives of a composed action. Focus and caret placement
/// are asynchronous in most toolkits, so a click followed immediately by typing
/// can land in the previous field.
const COMPOSITE_STEP_GAP: Duration = Duration::from_millis(60);

/// Cmd+A on macOS, Ctrl+A elsewhere.
fn select_all(control: &Arc<dyn ComputerControl>) -> Result<()> {
    let modifier = if cfg!(target_os = "macos") {
        "command"
    } else {
        "ctrl"
    };
    control.hotkey(&[modifier.to_string(), "a".to_string()])
}

/// Clipboard side effects needed by [`paste_with_restore`].
///
/// Split out so the paste/restore ordering can be unit-tested without touching
/// the real clipboard.
trait ClipboardOps {
    fn replace_text(&mut self, text: &str) -> Result<Option<String>>;
    fn write_text(&mut self, text: &str) -> Result<()>;
}

struct SystemClipboard;

impl ClipboardOps for SystemClipboard {
    fn replace_text(&mut self, text: &str) -> Result<Option<String>> {
        crate::platform::clipboard::replace_text(text)
    }

    fn write_text(&mut self, text: &str) -> Result<()> {
        crate::platform::clipboard::write_text(text)
    }
}

/// Paste `text`, then restore the previous clipboard content **after** the
/// target app has had time to read it.
///
/// The order is the whole point: restoring right after the paste hotkey makes
/// slow targets (Chrome's omnibox, Electron editors, remote desktops) paste the
/// *previous* content. Measured on macOS TextEdit: 0/5/10 ms always pasted the
/// old text, 30 ms still lost 10/30, 50 ms was 30/30 — hence the configurable
/// `computer_paste_restore_delay_ms` (floored at 50 ms).
///
/// The previous content is restored even when the paste fails, so a failed
/// paste does not leave our text sitting on the user's clipboard.
fn paste_with_restore(
    clipboard: &mut dyn ClipboardOps,
    text: &str,
    restore_delay_ms: u64,
    sleep: &mut dyn FnMut(u64),
    paste: &mut dyn FnMut() -> Result<()>,
) -> Result<()> {
    let saved = clipboard.replace_text(text)?;
    let out = paste();
    if out.is_ok() && restore_delay_ms > 0 {
        sleep(restore_delay_ms);
    }
    if let Some(prev) = saved {
        let _ = clipboard.write_text(&prev);
    }
    out
}

/// Paste through the clipboard, restoring the user's previous content.
///
/// Character-by-character typing goes through the IME and mangles CJK input, so
/// the clipboard is the default path. The pasteboard itself is touched only via
/// [`crate::platform::clipboard`], which keeps it on the main thread.
fn paste_via_clipboard(control: &Arc<dyn ComputerControl>, text: &str) -> Result<()> {
    let modifier = if cfg!(target_os = "macos") {
        "command"
    } else {
        "ctrl"
    };
    let restore_delay_ms = u64::from(
        crate::platform_config::effective_settings_global().computer_paste_restore_delay_ms,
    );
    let mut sleep = |ms: u64| std::thread::sleep(Duration::from_millis(ms));
    let mut paste = || control.hotkey(&[modifier.to_string(), "v".to_string()]);
    paste_with_restore(
        &mut SystemClipboard,
        text,
        restore_delay_ms,
        &mut sleep,
        &mut paste,
    )
}

/// Type into whatever has focus: optional select-all, then text, then Enter.
///
/// Text always goes through the clipboard. `enigo.text()` — the character-by-
/// character path — is intercepted by an active IME: with a Chinese input
/// method focused, letters land in the composition buffer instead of the field.
/// That is what `use_clipboard` used to switch back to, and the model reached
/// for it often enough to bring the bug back; the option is gone.
fn type_text(
    control: &Arc<dyn ComputerControl>,
    text: &str,
    clear_first: bool,
    auto_enter: bool,
) -> Result<()> {
    if clear_first {
        select_all(control)?;
        std::thread::sleep(COMPOSITE_STEP_GAP);
    }
    paste_via_clipboard(control, text)?;
    if auto_enter {
        control.press("return")?;
    }
    Ok(())
}

// ── InputAtTool (`input_at`) ─────────────────────────────────────────────────

/// Type text at logical screen pixel coordinates.
pub struct InputAtTool {
    control: Arc<dyn ComputerControl>,
    vision_state: Arc<Mutex<VisionState>>,
    verify: VerifyHintGenerator,
}

impl InputAtTool {
    pub fn new(control: Arc<dyn ComputerControl>, vision_state: Arc<Mutex<VisionState>>) -> Self {
        Self {
            control,
            vision_state,
            verify: VerifyHintGenerator::new(),
        }
    }

    pub fn execute(&self, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        let x = required_f32_arg(args, "x")?;
        let y = required_f32_arg(args, "y")?;
        let text = text_from_args(args.get("text"))?;
        let clear_first = json_bool_loose(args.get("clear_first"));
        let auto_enter = json_bool_loose(args.get("auto_enter"));
        let vision = self.vision_state.lock().unwrap();
        let (px, py) = vision
            .resolve_coordinate(x, y)
            .ok_or_else(|| anyhow::anyhow!("Screen bounds not set"))?;
        drop(vision);
        self.control
            .click(MouseButton::Left, Some(px), Some(py), 1)?;
        std::thread::sleep(COMPOSITE_STEP_GAP);
        type_text(&self.control, &text, clear_first, auto_enter)?;
        Ok(self.verify.type_hint(&text))
    }
}

// ── InputFocusedTool (`input_focused`) ───────────────────────────────────────

/// Type text into the currently focused input field.
pub struct InputFocusedTool {
    control: Arc<dyn ComputerControl>,
    verify: VerifyHintGenerator,
}

impl InputFocusedTool {
    pub fn new(control: Arc<dyn ComputerControl>) -> Self {
        Self {
            control,
            verify: VerifyHintGenerator::new(),
        }
    }

    pub fn execute(&self, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        let text = text_from_args(args.get("text"))?;
        let clear_first = json_bool_loose(args.get("clear_first"));
        let auto_enter = json_bool_loose(args.get("auto_enter"));
        type_text(&self.control, &text, clear_first, auto_enter)?;
        Ok(self.verify.type_hint(&text))
    }
}

// ── InputTool ────────────────────────────────────────────────────────────────

pub struct InputTool {
    at: InputAtTool,
    focused: InputFocusedTool,
}

impl InputTool {
    pub fn new(control: Arc<dyn ComputerControl>, vision_state: Arc<Mutex<VisionState>>) -> Self {
        Self {
            at: InputAtTool::new(control.clone(), vision_state),
            focused: InputFocusedTool::new(control),
        }
    }

    pub fn execute_with(&self, backend: InputBackend, args: &Value) -> Result<String> {
        match backend {
            InputBackend::At => self.at.execute(args),
            InputBackend::Focused => self.focused.execute(args),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct FakeClipboard {
        log: Rc<RefCell<Vec<String>>>,
        previous: Option<String>,
    }

    impl ClipboardOps for FakeClipboard {
        fn replace_text(&mut self, text: &str) -> Result<Option<String>> {
            self.log.borrow_mut().push(format!("replace:{text}"));
            Ok(self.previous.clone())
        }

        fn write_text(&mut self, text: &str) -> Result<()> {
            self.log.borrow_mut().push(format!("restore:{text}"));
            Ok(())
        }
    }

    #[test]
    fn restores_clipboard_only_after_the_paste_delay() {
        let log = Rc::new(RefCell::new(Vec::<String>::new()));
        let mut clipboard = FakeClipboard {
            log: log.clone(),
            previous: Some("old".into()),
        };
        let mut sleep = {
            let log = log.clone();
            move |ms: u64| log.borrow_mut().push(format!("sleep:{ms}"))
        };
        let mut paste = {
            let log = log.clone();
            move || {
                log.borrow_mut().push("paste".into());
                Ok(())
            }
        };

        paste_with_restore(&mut clipboard, "new", 50, &mut sleep, &mut paste).unwrap();

        // Restoring before the sleep would make slow targets paste `old`.
        assert_eq!(
            *log.borrow(),
            vec![
                "replace:new".to_string(),
                "paste".to_string(),
                "sleep:50".to_string(),
                "restore:old".to_string(),
            ]
        );
    }

    #[test]
    fn failed_paste_restores_clipboard_without_waiting() {
        let log = Rc::new(RefCell::new(Vec::<String>::new()));
        let mut clipboard = FakeClipboard {
            log: log.clone(),
            previous: Some("old".into()),
        };
        let mut sleep = {
            let log = log.clone();
            move |ms: u64| log.borrow_mut().push(format!("sleep:{ms}"))
        };
        let mut paste = {
            let log = log.clone();
            move || {
                log.borrow_mut().push("paste".into());
                Err(anyhow::anyhow!("paste hotkey failed"))
            }
        };

        assert!(paste_with_restore(&mut clipboard, "new", 50, &mut sleep, &mut paste).is_err());

        assert_eq!(
            *log.borrow(),
            vec![
                "replace:new".to_string(),
                "paste".to_string(),
                "restore:old".to_string(),
            ],
            "a failed paste must not leave our text on the clipboard"
        );
    }
}
