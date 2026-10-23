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

/// Paste through the clipboard, restoring the user's previous content.
///
/// Character-by-character typing goes through the IME and mangles CJK input, so
/// the clipboard is the default path. The pasteboard itself is touched only via
/// [`crate::platform::clipboard`], which keeps it on the main thread.
fn paste_via_clipboard(control: &Arc<dyn ComputerControl>, text: &str) -> Result<()> {
    let saved = crate::platform::clipboard::replace_text(text)?;
    // The OS publishes the new clipboard asynchronously; pasting too early gets
    // the previous content.
    std::thread::sleep(Duration::from_millis(50));
    let modifier = if cfg!(target_os = "macos") {
        "command"
    } else {
        "ctrl"
    };
    let out = control.hotkey(&[modifier.to_string(), "v".to_string()]);
    if let Some(prev) = saved {
        let _ = crate::platform::clipboard::write_text(&prev);
    }
    out
}

/// Type into whatever has focus: optional select-all, then text, then Enter.
fn type_text(
    control: &Arc<dyn ComputerControl>,
    text: &str,
    clear_first: bool,
    auto_enter: bool,
    use_clipboard: bool,
) -> Result<()> {
    if clear_first {
        select_all(control)?;
        std::thread::sleep(COMPOSITE_STEP_GAP);
    }
    if use_clipboard {
        paste_via_clipboard(control, text)?;
    } else {
        control.typing(text)?;
    }
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
        let use_clipboard = args
            .get("use_clipboard")
            .map_or(true, |v| json_bool_loose(Some(v)));
        let vision = self.vision_state.lock().unwrap();
        let (px, py) = vision
            .resolve_coordinate(x, y)
            .ok_or_else(|| anyhow::anyhow!("Screen bounds not set"))?;
        drop(vision);
        self.control
            .click(MouseButton::Left, Some(px), Some(py), 1)?;
        std::thread::sleep(COMPOSITE_STEP_GAP);
        type_text(
            &self.control,
            &text,
            clear_first,
            auto_enter,
            use_clipboard,
        )?;
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
        let use_clipboard = args
            .get("use_clipboard")
            .map_or(true, |v| json_bool_loose(Some(v)));
        type_text(
            &self.control,
            &text,
            clear_first,
            auto_enter,
            use_clipboard,
        )?;
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
