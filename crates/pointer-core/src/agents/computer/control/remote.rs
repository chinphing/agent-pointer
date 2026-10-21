//! `RemoteControl`: compiles [`ComputerControl`] calls into OSWorld pyautogui
//! statements and hands them to an injected transport.
//!
//! Script generation is **pure** and unit-tested; the transport is a trait so
//! the HTTP client can be added without touching the action mapping.
//!
//! # Deviations from OSWorld `execute_action` (verify before evaluation)
//!
//! | action | OSWorld | here | why |
//! |---|---|---|---|
//! | `MOVE_TO` | `moveTo(x, y, duration=, tween=easeInQuad)` | `moveTo(x, y)` | the VM needs no human-like easing; no artificial delay |
//! | `WAIT` | no-op | no script | OSWorld treats it as a terminal signal; pacing lives in the tool layer |
//!
//! Everything else is emitted one-for-one.

use anyhow::Result;

use super::ComputerControl;
use crate::agents::computer::input::actions::MouseButton;

/// Executes generated scripts and answers observation calls on the remote host.
pub trait RemoteTransport: Send + Sync {
    /// Run one pyautogui script on the VM.
    fn execute(&self, script: &str) -> Result<()>;
    /// Current screen as JPEG bytes.
    fn screenshot(&self) -> Result<Vec<u8>>;
    /// Screen size in pixels.
    fn screen_size(&self) -> Result<(u32, u32)>;
    /// Pointer position in screen pixels.
    fn cursor_position(&self) -> Result<(i32, i32)>;
}

/// Remote implementation of [`ComputerControl`].
pub struct RemoteControl {
    transport: Box<dyn RemoteTransport>,
}

impl RemoteControl {
    pub fn new(transport: Box<dyn RemoteTransport>) -> Self {
        Self { transport }
    }

    fn run(&self, script: String) -> Result<()> {
        if script.is_empty() {
            return Ok(());
        }
        self.transport.execute(&script)
    }
}

fn button_name(button: MouseButton) -> &'static str {
    button.as_str()
}

/// Python string literal for `text`, escaping the characters that would
/// otherwise break the generated script.
pub fn python_str(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('\'');
    out
}

/// `MOVE_TO`.
pub fn move_to_script(x: i32, y: i32) -> String {
    format!("pyautogui.moveTo({x}, {y})")
}

/// `CLICK`. `x`/`y` are omitted when the click targets the current pointer.
pub fn click_script(
    button: MouseButton,
    x: Option<i32>,
    y: Option<i32>,
    clicks: u32,
) -> String {
    let mut args = format!("button='{}'", button_name(button));
    if let (Some(x), Some(y)) = (x, y) {
        args.push_str(&format!(", x={x}, y={y}"));
    }
    if clicks > 1 {
        args.push_str(&format!(", clicks={clicks}"));
    }
    format!("pyautogui.click({args})")
}

/// `DRAG_TO` — always from the current pointer, left button.
pub fn drag_to_script(x: i32, y: i32) -> String {
    format!("pyautogui.dragTo({x}, {y}, duration=1.0, button='left', mouseDownUp=True)")
}

/// `SCROLL` — one statement per non-zero axis.
pub fn scroll_script(dx: i32, dy: i32) -> String {
    let mut parts: Vec<String> = Vec::new();
    if dx != 0 {
        parts.push(format!("pyautogui.hscroll({dx})"));
    }
    if dy != 0 {
        parts.push(format!("pyautogui.vscroll({dy})"));
    }
    parts.join("\n")
}

/// `HOTKEY`.
pub fn hotkey_script(keys: &[String]) -> String {
    let args: Vec<String> = keys.iter().map(|k| python_str(k)).collect();
    format!("pyautogui.hotkey({})", args.join(", "))
}

impl ComputerControl for RemoteControl {
    fn move_to(&self, x: i32, y: i32) -> Result<()> {
        self.run(move_to_script(x, y))
    }

    fn click(
        &self,
        button: MouseButton,
        x: Option<i32>,
        y: Option<i32>,
        clicks: u32,
    ) -> Result<()> {
        if clicks == 0 {
            anyhow::bail!("clicks must be at least 1");
        }
        self.run(click_script(button, x, y, clicks))
    }

    fn mouse_down(&self, button: MouseButton) -> Result<()> {
        self.run(format!("pyautogui.mouseDown(button='{}')", button_name(button)))
    }

    fn mouse_up(&self, button: MouseButton) -> Result<()> {
        self.run(format!("pyautogui.mouseUp(button='{}')", button_name(button)))
    }

    fn right_click(&self, x: i32, y: i32) -> Result<()> {
        self.run(format!("pyautogui.rightClick(x={x}, y={y})"))
    }

    fn double_click(&self, x: i32, y: i32) -> Result<()> {
        self.run(format!("pyautogui.doubleClick(x={x}, y={y})"))
    }

    fn drag_to(&self, x: i32, y: i32) -> Result<()> {
        self.run(drag_to_script(x, y))
    }

    fn scroll(&self, dx: i32, dy: i32) -> Result<()> {
        self.run(scroll_script(dx, dy))
    }

    fn typing(&self, text: &str) -> Result<()> {
        self.run(format!("pyautogui.typewrite({})", python_str(text)))
    }

    fn press(&self, key: &str) -> Result<()> {
        self.run(format!("pyautogui.press({})", python_str(key)))
    }

    fn key_down(&self, key: &str) -> Result<()> {
        self.run(format!("pyautogui.keyDown({})", python_str(key)))
    }

    fn key_up(&self, key: &str) -> Result<()> {
        self.run(format!("pyautogui.keyUp({})", python_str(key)))
    }

    fn release_stranded_input(&self) -> Result<()> {
        let script = [
            "import pyautogui as _pg",
            "for _k in ('ctrl', 'alt', 'shift', 'cmd'):",
            "    try:",
            "        _pg.keyUp(_k)",
            "    except Exception:",
            "        pass",
            "for _b in ('left', 'middle', 'right'):",
            "    try:",
            "        _pg.mouseUp(button=_b)",
            "    except Exception:",
            "        pass",
        ]
        .join("\n");
        self.run(script)
    }

    fn hotkey(&self, keys: &[String]) -> Result<()> {
        if keys.is_empty() {
            anyhow::bail!("hotkey needs at least one key");
        }
        self.run(hotkey_script(keys))
    }

    /// `WAIT` is a terminal signal in OSWorld; pacing belongs to the tool layer.
    fn wait(&self, _ms: u64) -> Result<()> {
        Ok(())
    }

    fn screenshot(&self) -> Result<Vec<u8>> {
        self.transport.screenshot()
    }

    fn screen_size(&self) -> Result<(u32, u32)> {
        self.transport.screen_size()
    }

    fn cursor_position(&self) -> Result<(i32, i32)> {
        self.transport.cursor_position()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingTransport {
        scripts: Arc<Mutex<Vec<String>>>,
    }

    impl RemoteTransport for RecordingTransport {
        fn execute(&self, script: &str) -> Result<()> {
            self.scripts.lock().unwrap().push(script.to_string());
            Ok(())
        }
        fn screenshot(&self) -> Result<Vec<u8>> {
            Ok(vec![1, 2, 3])
        }
        fn screen_size(&self) -> Result<(u32, u32)> {
            Ok((1920, 1080))
        }
        fn cursor_position(&self) -> Result<(i32, i32)> {
            Ok((7, 9))
        }
    }

    fn control() -> (RemoteControl, Arc<Mutex<Vec<String>>>) {
        let t = RecordingTransport::default();
        let scripts = t.scripts.clone();
        (RemoteControl::new(Box::new(t)), scripts)
    }

    fn only_script(scripts: &Arc<Mutex<Vec<String>>>) -> String {
        let s = scripts.lock().unwrap();
        assert_eq!(s.len(), 1, "expected one script, got {s:?}");
        s[0].clone()
    }

    #[test]
    fn move_to_matches_osworld_shape() {
        let (c, s) = control();
        c.move_to(10, 20).unwrap();
        assert_eq!(only_script(&s), "pyautogui.moveTo(10, 20)");
    }

    #[test]
    fn click_covers_positioned_and_bare_forms() {
        let (c, s) = control();
        c.click(MouseButton::Left, Some(1), Some(2), 1).unwrap();
        assert_eq!(
            only_script(&s),
            "pyautogui.click(button='left', x=1, y=2)"
        );

        let (c, s) = control();
        c.click(MouseButton::Left, Some(1), Some(2), 2).unwrap();
        assert_eq!(
            only_script(&s),
            "pyautogui.click(button='left', x=1, y=2, clicks=2)"
        );

        let (c, s) = control();
        c.click(MouseButton::Right, None, None, 1).unwrap();
        assert_eq!(only_script(&s), "pyautogui.click(button='right')");
    }

    #[test]
    fn click_rejects_zero_clicks() {
        let (c, _) = control();
        assert!(c.click(MouseButton::Left, Some(1), Some(2), 0).is_err());
    }

    #[test]
    fn mouse_phase_and_key_phase_emit_named_calls() {
        let (c, s) = control();
        c.mouse_down(MouseButton::Left).unwrap();
        c.mouse_up(MouseButton::Right).unwrap();
        c.key_down("shift").unwrap();
        c.key_up("shift").unwrap();
        let log = s.lock().unwrap().clone();
        assert_eq!(
            log,
            vec![
                "pyautogui.mouseDown(button='left')".to_string(),
                "pyautogui.mouseUp(button='right')".to_string(),
                "pyautogui.keyDown('shift')".to_string(),
                "pyautogui.keyUp('shift')".to_string(),
            ]
        );
    }

    #[test]
    fn right_and_double_click_emit_dedicated_calls() {
        let (c, s) = control();
        c.right_click(3, 4).unwrap();
        assert_eq!(only_script(&s), "pyautogui.rightClick(x=3, y=4)");

        let (c, s) = control();
        c.double_click(3, 4).unwrap();
        assert_eq!(only_script(&s), "pyautogui.doubleClick(x=3, y=4)");
    }

    #[test]
    fn drag_to_uses_current_pointer_semantics() {
        let (c, s) = control();
        c.drag_to(30, 40).unwrap();
        assert_eq!(
            only_script(&s),
            "pyautogui.dragTo(30, 40, duration=1.0, button='left', mouseDownUp=True)"
        );
    }

    #[test]
    fn scroll_emits_one_statement_per_axis() {
        assert_eq!(scroll_script(0, -3), "pyautogui.vscroll(-3)");
        assert_eq!(scroll_script(5, 0), "pyautogui.hscroll(5)");
        assert_eq!(
            scroll_script(5, -3),
            "pyautogui.hscroll(5)\npyautogui.vscroll(-3)"
        );
        assert_eq!(scroll_script(0, 0), "");
    }

    #[test]
    fn scroll_with_no_axis_sends_nothing() {
        let (c, s) = control();
        c.scroll(0, 0).unwrap();
        assert!(s.lock().unwrap().is_empty());
    }

    #[test]
    fn typing_escapes_quotes_and_newlines() {
        let (c, s) = control();
        c.typing("it's\nfine\\ok").unwrap();
        assert_eq!(
            only_script(&s),
            "pyautogui.typewrite('it\\'s\\nfine\\\\ok')"
        );
    }

    #[test]
    fn hotkey_quotes_each_key() {
        let (c, s) = control();
        c.hotkey(&["command".to_string(), "c".to_string()]).unwrap();
        assert_eq!(only_script(&s), "pyautogui.hotkey('command', 'c')");
    }

    #[test]
    fn hotkey_needs_at_least_one_key() {
        let (c, _) = control();
        assert!(c.hotkey(&[]).is_err());
    }

    #[test]
    fn wait_emits_no_script() {
        let (c, s) = control();
        c.wait(500).unwrap();
        assert!(s.lock().unwrap().is_empty());
    }

    #[test]
    fn observation_delegates_to_the_transport() {
        let (c, _) = control();
        assert_eq!(c.screen_size().unwrap(), (1920, 1080));
        assert_eq!(c.cursor_position().unwrap(), (7, 9));
        assert_eq!(c.screenshot().unwrap(), vec![1, 2, 3]);
    }

    /// Every one of the 13 `computer_13` actions, one row per action, in the
    /// order of design §4.1. This is the alignment contract: the emitted
    /// pyautogui statement must match the table verbatim.
    #[test]
    fn all_thirteen_actions_match_the_osworld_table() {
        struct Row {
            action: &'static str,
            expected: &'static str,
            call: fn(&RemoteControl),
        }

        let rows = [
            Row {
                action: "MOVE_TO",
                expected: "pyautogui.moveTo(120, 240)",
                call: |c| c.move_to(120, 240).unwrap(),
            },
            Row {
                action: "CLICK",
                expected: "pyautogui.click(button='left', x=120, y=240)",
                call: |c| c.click(MouseButton::Left, Some(120), Some(240), 1).unwrap(),
            },
            Row {
                action: "MOUSE_DOWN",
                expected: "pyautogui.mouseDown(button='left')",
                call: |c| c.mouse_down(MouseButton::Left).unwrap(),
            },
            Row {
                action: "MOUSE_UP",
                expected: "pyautogui.mouseUp(button='left')",
                call: |c| c.mouse_up(MouseButton::Left).unwrap(),
            },
            Row {
                action: "RIGHT_CLICK",
                expected: "pyautogui.rightClick(x=120, y=240)",
                call: |c| c.right_click(120, 240).unwrap(),
            },
            Row {
                action: "DOUBLE_CLICK",
                expected: "pyautogui.doubleClick(x=120, y=240)",
                call: |c| c.double_click(120, 240).unwrap(),
            },
            Row {
                action: "DRAG_TO",
                expected: "pyautogui.dragTo(120, 240, duration=1.0, button='left', mouseDownUp=True)",
                call: |c| c.drag_to(120, 240).unwrap(),
            },
            Row {
                action: "SCROLL",
                expected: "pyautogui.hscroll(30)\npyautogui.vscroll(-40)",
                call: |c| c.scroll(30, -40).unwrap(),
            },
            Row {
                action: "TYPING",
                expected: "pyautogui.typewrite('hi')",
                call: |c| c.typing("hi").unwrap(),
            },
            Row {
                action: "PRESS",
                expected: "pyautogui.press('enter')",
                call: |c| c.press("enter").unwrap(),
            },
            Row {
                action: "KEY_DOWN",
                expected: "pyautogui.keyDown('shift')",
                call: |c| c.key_down("shift").unwrap(),
            },
            Row {
                action: "KEY_UP",
                expected: "pyautogui.keyUp('shift')",
                call: |c| c.key_up("shift").unwrap(),
            },
            Row {
                action: "HOTKEY",
                expected: "pyautogui.hotkey('ctrl', 'c')",
                call: |c| c.hotkey(&["ctrl".to_string(), "c".to_string()]).unwrap(),
            },
        ];

        assert_eq!(rows.len(), 13, "the action set is 13, not more");
        for row in rows {
            let (c, scripts) = control();
            (row.call)(&c);
            let emitted = only_script(&scripts);
            assert_eq!(emitted, row.expected, "action {} drifted", row.action);
        }
    }

    #[test]
    fn all_three_buttons_reach_pyautogui() {
        for (button, name) in [
            (MouseButton::Left, "left"),
            (MouseButton::Middle, "middle"),
            (MouseButton::Right, "right"),
        ] {
            let (c, s) = control();
            c.mouse_down(button).unwrap();
            assert_eq!(only_script(&s), format!("pyautogui.mouseDown(button='{name}')"));

            let (c, s) = control();
            c.mouse_up(button).unwrap();
            assert_eq!(only_script(&s), format!("pyautogui.mouseUp(button='{name}')"));

            let (c, s) = control();
            c.click(button, Some(3), Some(4), 1).unwrap();
            assert_eq!(
                only_script(&s),
                format!("pyautogui.click(button='{name}', x=3, y=4)")
            );
        }
    }

    #[test]
    fn click_forwards_click_counts_above_one() {
        let (c, s) = control();
        c.click(MouseButton::Middle, Some(3), Some(4), 2).unwrap();
        assert_eq!(
            only_script(&s),
            "pyautogui.click(button='middle', x=3, y=4, clicks=2)"
        );
    }
}
