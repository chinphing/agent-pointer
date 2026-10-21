pub use super::mouse_move::MouseMoveProfile;
use super::timing::{
    COMPOSITE_ACTION_STEP_GAP_MS, POST_MOUSE_BUTTON_SETTLE_MS, SETTLE_AFTER_ABSOLUTE_MOVE_MS,
};
use anyhow::{anyhow, Result};
use std::time::Duration;

/// After an absolute `move_to`, wait briefly so the OS / target app can update hit-testing (hover,
/// window activation, text-field focus, etc.) before the next click or scroll.
#[inline]
fn settle_after_absolute_move() {
    std::thread::sleep(Duration::from_millis(SETTLE_AFTER_ABSOLUTE_MOVE_MS));
}

/// After left / right / double click so the target app can process the button event.
#[inline]
fn settle_after_mouse_button() {
    std::thread::sleep(Duration::from_millis(POST_MOUSE_BUTTON_SETTLE_MS));
}

/// Between composite sub-steps (focus → type, select-all → type, position → scroll); see [`COMPOSITE_ACTION_STEP_GAP_MS`].
#[inline]
fn composite_step_gap() {
    std::thread::sleep(Duration::from_millis(COMPOSITE_ACTION_STEP_GAP_MS));
}

/// Mouse button for low-level press/release.
///
/// Mirrors OSWorld's `button` parameter (`left` / `middle` / `right`); the
/// three-value set is required by the `MOUSE_DOWN` / `MOUSE_UP` / `CLICK`
/// actions (design §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

impl MouseButton {
    /// Parse an OSWorld-style button name (`left` / `middle` / `right`).
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "left" => Some(Self::Left),
            "middle" => Some(Self::Middle),
            "right" => Some(Self::Right),
            _ => None,
        }
    }

    /// OSWorld / pyautogui button name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Middle => "middle",
            Self::Right => "right",
        }
    }
}

/// Pixels per wheel line for the pixel-based `SCROLL` contract.
///
/// `ComputerControl::scroll` takes **pixels** (design §7.3 / §10 #2); the local
/// backend's only primitive is wheel lines, so it divides by this factor. The
/// remote backend forwards pixels unchanged to `pyautogui.hscroll/vscroll`.
pub const SCROLL_PIXELS_PER_LINE: i32 = 100;

/// Convert a pixel scroll delta into whole wheel lines.
///
/// Sign is preserved and a non-zero pixel delta never collapses to zero lines,
/// so a small scroll still moves the viewport by one notch.
pub fn scroll_lines_for_pixels(pixels: i32) -> i32 {
    if pixels == 0 {
        return 0;
    }
    let magnitude =
        (pixels.unsigned_abs() as f64 / SCROLL_PIXELS_PER_LINE as f64).round() as i32;
    magnitude.max(1) * pixels.signum()
}

/// Key or mouse button press vs release (maps to enigo Press/Release).
#[derive(Debug, Clone, Copy)]
pub enum KeyPhase {
    Press,
    Release,
}

/// Result of an action execution.
#[derive(Debug, Clone)]
pub struct ActionResult {
    /// Whether the action succeeded.
    pub success: bool,
    /// Optional message describing the result.
    pub message: String,
}

impl ActionResult {
    /// Create a successful action result.
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
        }
    }

    /// Create a failed action result.
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
        }
    }
}

/// Cross-platform action backend abstraction.
///
/// This trait defines the low-level operations that can be performed
/// on the host system. Implementations can use different underlying
/// libraries (e.g., enigo, core-graphics, etc.).
pub trait ActionBackend: Send + Sync {
    /// Perform a single click at the current cursor position.
    fn click(&self) -> Result<ActionResult>;

    /// Perform a double-click at the current cursor position.
    fn double_click(&self) -> Result<ActionResult>;

    /// Perform a right-click at the current cursor position.
    fn right_click(&self) -> Result<ActionResult>;

    /// Move the cursor to the specified screen coordinates (default profile: fast).
    fn move_to(&self, x: i32, y: i32) -> Result<ActionResult> {
        self.move_to_with_profile(x, y, MouseMoveProfile::default())
    }

    /// Move the cursor using a movement profile (path + timing + jitter).
    fn move_to_with_profile(
        &self,
        x: i32,
        y: i32,
        profile: MouseMoveProfile,
    ) -> Result<ActionResult>;

    /// Scroll the mouse wheel.
    ///
    /// # Arguments
    /// * `lines` - Number of lines to scroll (positive = up, negative = down).
    fn scroll(&self, lines: i32) -> Result<ActionResult>;

    /// Scroll the mouse wheel horizontally.
    ///
    /// # Arguments
    /// * `lines` - Number of lines to scroll (positive = right, negative = left).
    ///
    /// Backends that cannot express horizontal scrolling keep the default
    /// error rather than silently dropping the axis.
    fn scroll_horizontal(&self, lines: i32) -> Result<ActionResult> {
        Err(anyhow!(
            "horizontal scroll is not supported by this backend (requested {lines} lines)"
        ))
    }

    /// Type the given text.
    ///
    /// # Arguments
    /// * `text` - The text to type.
    fn type_text(&self, text: &str) -> Result<ActionResult>;

    /// Type text by clipboard write + paste (bypasses IME).
    /// Default impl falls back to type_text.
    fn type_text_via_clipboard(&self, text: &str) -> Result<ActionResult> {
        self.type_text(text)
    }

    /// Execute a hotkey combination.
    ///
    /// # Arguments
    /// * `keys` - A slice of key names (e.g., &["command", "c"]).
    fn hotkey(&self, keys: &[&str]) -> Result<ActionResult>;

    /// Get the current cursor position.
    ///
    /// # Returns
    /// A tuple of (x, y) screen coordinates in pixels.
    fn get_position(&self) -> Result<(i32, i32)>;

    /// Press or release a named key (same names as [`hotkey`](ActionBackend::hotkey)).
    fn key_phase(&self, name: &str, phase: KeyPhase) -> Result<ActionResult>;

    /// Press or release a mouse button at the **current** cursor position.
    fn mouse_phase(&self, button: MouseButton, phase: KeyPhase) -> Result<ActionResult>;
}

/// High-level action executor that holds a backend.
///
/// This struct provides convenience methods for common action patterns
/// and handles the two positioning systems (index-based and coordinate-based).
pub struct ActionExecutor {
    backend: Box<dyn ActionBackend>,
}

impl std::fmt::Debug for ActionExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionExecutor").finish_non_exhaustive()
    }
}

impl ActionExecutor {
    /// Create a new ActionExecutor with the given backend.
    pub fn new(backend: Box<dyn ActionBackend>) -> Self {
        Self { backend }
    }

    /// Left-click at the **current** cursor without moving (Pointer `click_current`).
    pub fn click_here(&self) -> Result<ActionResult> {
        self.backend.click()?;
        settle_after_mouse_button();
        Ok(ActionResult::success("Clicked"))
    }

    /// Double-click at the current cursor.
    pub fn double_click_here(&self) -> Result<ActionResult> {
        self.backend.double_click()?;
        settle_after_mouse_button();
        Ok(ActionResult::success("Double-clicked"))
    }

    /// Right-click at the current cursor.
    pub fn right_click_here(&self) -> Result<ActionResult> {
        self.backend.right_click()?;
        settle_after_mouse_button();
        Ok(ActionResult::success("Right-clicked"))
    }

    /// Drag with left button from pixel (x1,y1) to (x2,y2).
    pub fn drag_left_from_to(
        &self,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        human_like: bool,
    ) -> Result<ActionResult> {
        if x1 == x2 && y1 == y2 {
            return Err(anyhow!("drag start and end must differ"));
        }
        let to_start = if human_like {
            MouseMoveProfile::drag_to_start()
        } else {
            MouseMoveProfile::standard()
        };
        self.backend.move_to_with_profile(x1, y1, to_start)?;
        settle_after_absolute_move();
        std::thread::sleep(Duration::from_millis(50));
        self.backend
            .mouse_phase(MouseButton::Left, KeyPhase::Press)?;
        std::thread::sleep(Duration::from_millis(50));
        self.backend
            .move_to_with_profile(x2, y2, MouseMoveProfile::drag_segment(human_like))?;
        std::thread::sleep(Duration::from_millis(50));
        self.backend
            .mouse_phase(MouseButton::Left, KeyPhase::Release)?;
        settle_after_mouse_button();
        Ok(ActionResult::success("drag completed"))
    }

    /// Drag with left button from the **current** pointer to (x, y) (`DRAG_TO`).
    ///
    /// OSWorld's `dragTo` presses at the current position; the pointer is not
    /// re-targeted to a separate start point.
    pub fn drag_left_from_current_to(
        &self,
        x: i32,
        y: i32,
        human_like: bool,
    ) -> Result<ActionResult> {
        let (x0, y0) = self.backend.get_position()?;
        self.drag_left_from_to(x0, y0, x, y, human_like)
    }

    /// Drag with left button through multiple already-resolved screen-pixel waypoints.
    pub fn drag_left_through_points(
        &self,
        points: &[(i32, i32)],
        human_like: bool,
    ) -> Result<ActionResult> {
        if points.len() < 2 {
            return Err(anyhow!("drag requires at least two points"));
        }
        let (x1, y1) = points[0];
        let to_start = if human_like {
            MouseMoveProfile::drag_to_start()
        } else {
            MouseMoveProfile::standard()
        };
        self.backend.move_to_with_profile(x1, y1, to_start)?;
        settle_after_absolute_move();
        std::thread::sleep(Duration::from_millis(50));
        self.backend
            .mouse_phase(MouseButton::Left, KeyPhase::Press)?;
        std::thread::sleep(Duration::from_millis(50));
        for &(x, y) in &points[1..] {
            self.backend
                .move_to_with_profile(x, y, MouseMoveProfile::drag_segment(human_like))?;
        }
        std::thread::sleep(Duration::from_millis(50));
        self.backend
            .mouse_phase(MouseButton::Left, KeyPhase::Release)?;
        settle_after_mouse_button();
        Ok(ActionResult::success("drag completed"))
    }

    /// Hold primary multi-select modifier (Cmd on macOS, Ctrl elsewhere), click each pixel, release.
    pub fn click_add_to_selection_batch(
        &self,
        positions: &[(i32, i32)],
        human_like: bool,
    ) -> Result<ActionResult> {
        if positions.is_empty() {
            return Ok(ActionResult::success("no positions"));
        }
        let profile = move_profile_for_human_like(human_like);
        let meta = if cfg!(target_os = "macos") {
            "command"
        } else {
            "ctrl"
        };
        self.backend.key_phase(meta, KeyPhase::Press)?;
        std::thread::sleep(Duration::from_millis(50));
        for &(x, y) in positions {
            self.backend.move_to_with_profile(x, y, profile)?;
            settle_after_absolute_move();
            self.backend.click()?;
            settle_after_mouse_button();
            std::thread::sleep(Duration::from_millis(80));
        }
        self.backend.key_phase(meta, KeyPhase::Release)?;
        settle_after_mouse_button();
        Ok(ActionResult::success("multi-select clicks"))
    }

    /// Click first, then Shift+click second (range selection).
    pub fn click_range_shift(
        &self,
        first: (i32, i32),
        last: (i32, i32),
        human_like: bool,
    ) -> Result<ActionResult> {
        self.click_at(first.0, first.1, human_like)?;
        self.backend.key_phase("shift", KeyPhase::Press)?;
        std::thread::sleep(Duration::from_millis(50));
        self.click_at(last.0, last.1, human_like)?;
        self.backend.key_phase("shift", KeyPhase::Release)?;
        settle_after_mouse_button();
        Ok(ActionResult::success("shift range click"))
    }

    /// Click at the given screen coordinates (already resolved to pixels).
    ///
    /// Used by the **coordinate-based** positioning path.
    ///
    /// # Arguments
    /// * `x` - Screen X coordinate in pixels.
    /// * `y` - Screen Y coordinate in pixels.
    pub fn click_at(&self, x: i32, y: i32, human_like: bool) -> Result<ActionResult> {
        self.backend
            .move_to_with_profile(x, y, move_profile_for_human_like(human_like))?;
        settle_after_absolute_move();
        self.backend.click()?;
        settle_after_mouse_button();
        Ok(ActionResult::success("click"))
    }

    /// Click at an annotated element index.
    ///
    /// Caller passes resolved screen coordinates.
    /// Used by the **index-based** positioning path.
    ///
    /// # Arguments
    /// * `x` - Screen X coordinate in pixels (resolved from index).
    /// * `y` - Screen Y coordinate in pixels (resolved from index).
    pub fn click_index(&self, x: i32, y: i32, human_like: bool) -> Result<ActionResult> {
        self.click_at(x, y, human_like)
    }

    /// Double-click at the given screen coordinates.
    ///
    /// Used by the **coordinate-based** positioning path.
    pub fn double_click_at(&self, x: i32, y: i32, human_like: bool) -> Result<ActionResult> {
        self.backend
            .move_to_with_profile(x, y, move_profile_for_human_like(human_like))?;
        settle_after_absolute_move();
        self.backend.double_click()?;
        settle_after_mouse_button();
        Ok(ActionResult::success("double-click"))
    }

    /// Double-click at an annotated element index.
    ///
    /// Used by the **index-based** positioning path.
    pub fn double_click_index(&self, x: i32, y: i32, human_like: bool) -> Result<ActionResult> {
        self.double_click_at(x, y, human_like)
    }

    /// Right-click at the given screen coordinates.
    ///
    /// Used by the **coordinate-based** positioning path.
    pub fn right_click_at(&self, x: i32, y: i32, human_like: bool) -> Result<ActionResult> {
        self.backend
            .move_to_with_profile(x, y, move_profile_for_human_like(human_like))?;
        settle_after_absolute_move();
        self.backend.right_click()?;
        settle_after_mouse_button();
        Ok(ActionResult::success("right-click"))
    }

    /// Right-click at an annotated element index.
    ///
    /// Used by the **index-based** positioning path.
    pub fn right_click_index(&self, x: i32, y: i32, human_like: bool) -> Result<ActionResult> {
        self.right_click_at(x, y, human_like)
    }

    /// Click `button` at `(x, y)` with `clicks` press/release pairs (`CLICK`).
    ///
    /// Counts the backend cannot express are rejected instead of collapsed to
    /// one click: the caller must know the gesture did not happen as asked.
    pub fn click_button_at(
        &self,
        button: MouseButton,
        x: i32,
        y: i32,
        clicks: u32,
        human_like: bool,
    ) -> Result<ActionResult> {
        match (button, clicks) {
            (MouseButton::Left, 1) => self.click_at(x, y, human_like),
            (MouseButton::Left, 2) => self.double_click_at(x, y, human_like),
            (MouseButton::Right, 1) => self.right_click_at(x, y, human_like),
            (MouseButton::Middle, 1) => {
                self.backend
                    .move_to_with_profile(x, y, move_profile_for_human_like(human_like))?;
                settle_after_absolute_move();
                self.backend
                    .mouse_phase(MouseButton::Middle, KeyPhase::Press)?;
                self.backend
                    .mouse_phase(MouseButton::Middle, KeyPhase::Release)?;
                settle_after_mouse_button();
                Ok(ActionResult::success("middle-click"))
            }
            (button, clicks) => Err(anyhow!(
                "{} click with {clicks} clicks is not supported",
                button.as_str()
            )),
        }
    }

    /// Click `button` at the **current** pointer with `clicks` press/release pairs.
    pub fn click_button_here(
        &self,
        button: MouseButton,
        clicks: u32,
    ) -> Result<ActionResult> {
        match (button, clicks) {
            (MouseButton::Left, 1) => self.click_here(),
            (MouseButton::Left, 2) => self.double_click_here(),
            (MouseButton::Right, 1) => self.right_click_here(),
            (MouseButton::Middle, 1) => {
                self.backend
                    .mouse_phase(MouseButton::Middle, KeyPhase::Press)?;
                self.backend
                    .mouse_phase(MouseButton::Middle, KeyPhase::Release)?;
                settle_after_mouse_button();
                Ok(ActionResult::success("middle-click"))
            }
            (button, clicks) => Err(anyhow!(
                "{} click with {clicks} clicks is not supported",
                button.as_str()
            )),
        }
    }

    /// Move the cursor to the given screen coordinates without clicking.
    ///
    /// Used by the **coordinate-based** positioning path.
    pub fn hover_at(&self, x: i32, y: i32, human_like: bool) -> Result<ActionResult> {
        self.backend
            .move_to_with_profile(x, y, move_profile_for_human_like(human_like))
    }

    /// Move the cursor to an annotated element index.
    ///
    /// Used by the **index-based** positioning path.
    pub fn hover_index(&self, x: i32, y: i32, human_like: bool) -> Result<ActionResult> {
        self.hover_at(x, y, human_like)
    }

    /// Move the cursor by a relative offset.
    ///
    /// # Arguments
    /// * `dx` - Delta X in pixels.
    /// * `dy` - Delta Y in pixels.
    pub fn move_offset(&self, dx: i32, dy: i32, human_like: bool) -> Result<ActionResult> {
        let (current_x, current_y) = self.backend.get_position()?;
        self.backend.move_to_with_profile(
            current_x + dx,
            current_y + dy,
            move_profile_for_human_like(human_like),
        )
    }

    /// Scroll at the current cursor position.
    ///
    /// # Arguments
    /// * `lines` - Number of lines to scroll.
    pub fn scroll_at_current(&self, lines: i32) -> Result<ActionResult> {
        self.backend.scroll(lines)
    }

    /// Scroll at the current cursor position by **pixel** deltas (`SCROLL`).
    ///
    /// Positive `dy` scrolls up, positive `dx` scrolls right — the same sign
    /// convention as `pyautogui.vscroll` / `hscroll`, which the remote backend
    /// forwards verbatim. The local backend converts pixels to wheel lines
    /// (design §10 #2).
    pub fn scroll_at_current_pixels(&self, dx: i32, dy: i32) -> Result<ActionResult> {
        let vertical = scroll_lines_for_pixels(dy);
        if vertical != 0 {
            self.backend.scroll(vertical)?;
        }
        let horizontal = scroll_lines_for_pixels(dx);
        if horizontal != 0 {
            self.backend.scroll_horizontal(horizontal)?;
        }
        Ok(ActionResult::success(format!(
            "scrolled ({dx}, {dy}) px -> ({horizontal}, {vertical}) lines"
        )))
    }

    /// Scroll horizontally at the current cursor position (wheel lines).
    pub fn scroll_horizontal_at_current(&self, lines: i32) -> Result<ActionResult> {
        self.backend.scroll_horizontal(lines)
    }

    /// Type text at the current cursor position.
    ///
    /// # Arguments
    /// * `text` - The text to type.
    pub fn type_text(&self, text: &str) -> Result<ActionResult> {
        self.backend.type_text(text)
    }

    /// Execute a hotkey combination.
    ///
    /// # Arguments
    /// * `keys` - A slice of key names.
    pub fn hotkey(&self, keys: &[&str]) -> Result<ActionResult> {
        self.backend.hotkey(keys)
    }

    /// Get the current cursor position.
    pub fn get_position(&self) -> Result<(i32, i32)> {
        self.backend.get_position()
    }

    /// Type text at a specific location.
    ///
    /// Sequence: move → click → type.
    pub fn type_text_at(&self, x: i32, y: i32, text: &str) -> Result<ActionResult> {
        self.type_text_at_with_options(x, y, text, false, false, false, false)
    }

    /// Select-all hotkey for the current OS (Cmd+A / Ctrl+A).
    pub fn hotkey_select_all(&self) -> Result<ActionResult> {
        if cfg!(target_os = "macos") {
            self.hotkey(&["command", "a"])
        } else {
            self.hotkey(&["ctrl", "a"])
        }
    }

    /// Type at (x,y): optional select-all before typing, optional Enter after (Pointer composite).
    pub fn type_text_at_with_options(
        &self,
        x: i32,
        y: i32,
        text: &str,
        clear_first: bool,
        auto_enter: bool,
        human_like: bool,
        use_clipboard: bool,
    ) -> Result<ActionResult> {
        self.click_at(x, y, human_like)?;
        composite_step_gap();
        if clear_first {
            self.hotkey_select_all()?;
            composite_step_gap();
        }
        if use_clipboard {
            self.backend.type_text_via_clipboard(text)?;
        } else {
            self.backend.type_text(text)?;
        }
        if auto_enter {
            self.hotkey(&["return"])?;
        }
        Ok(ActionResult::success("typed"))
    }

    /// Type into focused field (no click): optional select-all, optional Enter.
    pub fn type_text_focused_with_options(
        &self,
        text: &str,
        clear_first: bool,
        auto_enter: bool,
        use_clipboard: bool,
    ) -> Result<ActionResult> {
        if clear_first {
            self.hotkey_select_all()?;
            composite_step_gap();
        }
        if use_clipboard {
            self.backend.type_text_via_clipboard(text)?;
        } else {
            self.backend.type_text(text)?;
        }
        if auto_enter {
            self.hotkey(&["return"])?;
        }
        Ok(ActionResult::success("typed focused"))
    }

    /// Scroll at a specific location.
    ///
    /// Sequence: move → scroll.
    pub fn scroll_at(&self, x: i32, y: i32, lines: i32, human_like: bool) -> Result<ActionResult> {
        self.backend
            .move_to_with_profile(x, y, move_profile_for_human_like(human_like))?;
        settle_after_absolute_move();
        composite_step_gap();
        self.backend.scroll(lines)
    }

    /// Move to a specific location without clicking.
    pub fn move_to(&self, x: i32, y: i32, human_like: bool) -> Result<ActionResult> {
        self.backend
            .move_to_with_profile(x, y, move_profile_for_human_like(human_like))
    }

    /// Press or release a mouse button at the **current** cursor position.
    ///
    /// Pass-through for the `MOUSE_DOWN` / `MOUSE_UP` control actions; the
    /// existing `click_*` helpers keep their own composite behaviour.
    pub fn mouse_phase(&self, button: MouseButton, phase: KeyPhase) -> Result<ActionResult> {
        let out = self.backend.mouse_phase(button, phase)?;
        settle_after_mouse_button();
        Ok(out)
    }

    /// Press or release a named key.
    pub fn key_phase(&self, name: &str, phase: KeyPhase) -> Result<ActionResult> {
        self.backend.key_phase(name, phase)
    }

    /// Press then release a named key (`PRESS`).
    pub fn press_key(&self, name: &str) -> Result<ActionResult> {
        self.backend.key_phase(name, KeyPhase::Press)?;
        composite_step_gap();
        self.backend.key_phase(name, KeyPhase::Release)
    }
}

#[inline]
fn move_profile_for_human_like(human_like: bool) -> MouseMoveProfile {
    if human_like {
        MouseMoveProfile::human_like()
    } else {
        MouseMoveProfile::standard()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A mock backend for testing.
    #[derive(Debug, Default)]
    struct MockBackend {
        actions: Arc<Mutex<Vec<String>>>,
        position: Arc<Mutex<(i32, i32)>>,
    }

    impl MockBackend {
        fn record(&self, action: impl Into<String>) {
            self.actions.lock().unwrap().push(action.into());
        }
    }

    impl ActionBackend for MockBackend {
        fn click(&self) -> Result<ActionResult> {
            self.record("click");
            Ok(ActionResult::success("clicked"))
        }

        fn double_click(&self) -> Result<ActionResult> {
            self.record("double_click");
            Ok(ActionResult::success("double clicked"))
        }

        fn right_click(&self) -> Result<ActionResult> {
            self.record("right_click");
            Ok(ActionResult::success("right clicked"))
        }

        fn move_to_with_profile(
            &self,
            x: i32,
            y: i32,
            profile: MouseMoveProfile,
        ) -> Result<ActionResult> {
            self.record(format!(
                "move_to_with_profile({},{},{:?})",
                x, y, profile.path.strategy
            ));
            *self.position.lock().unwrap() = (x, y);
            Ok(ActionResult::success("moved"))
        }

        fn scroll(&self, lines: i32) -> Result<ActionResult> {
            self.record(format!("scroll({})", lines));
            Ok(ActionResult::success("scrolled"))
        }

        fn scroll_horizontal(&self, lines: i32) -> Result<ActionResult> {
            self.record(format!("scroll_horizontal({})", lines));
            Ok(ActionResult::success("scrolled horizontally"))
        }

        fn type_text(&self, text: &str) -> Result<ActionResult> {
            self.record(format!("type_text({})", text));
            Ok(ActionResult::success("typed"))
        }

        fn type_text_via_clipboard(&self, text: &str) -> Result<ActionResult> {
            self.record(format!("type_text_via_clipboard({})", text));
            Ok(ActionResult::success("pasted via clipboard"))
        }

        fn hotkey(&self, keys: &[&str]) -> Result<ActionResult> {
            self.record(format!("hotkey({})", keys.join("+")));
            Ok(ActionResult::success("hotkey pressed"))
        }

        fn get_position(&self) -> Result<(i32, i32)> {
            Ok(*self.position.lock().unwrap())
        }

        fn key_phase(&self, name: &str, phase: KeyPhase) -> Result<ActionResult> {
            self.record(format!("key_phase({name},{phase:?})"));
            Ok(ActionResult::success("key"))
        }

        fn mouse_phase(&self, button: MouseButton, phase: KeyPhase) -> Result<ActionResult> {
            self.record(format!("mouse_phase({button:?},{phase:?})"));
            Ok(ActionResult::success("mouse phase"))
        }
    }

    #[test]
    fn test_click_at() {
        let backend = Box::new(MockBackend::default());
        let executor = ActionExecutor::new(backend);
        let result = executor.click_at(100, 200, false).unwrap();
        assert!(result.success);
    }

    #[test]
    fn test_click_index() {
        let backend = Box::new(MockBackend::default());
        let executor = ActionExecutor::new(backend);
        let result = executor.click_index(100, 200, false).unwrap();
        assert!(result.success);
    }

    #[test]
    fn test_move_offset() {
        let backend = Box::new(MockBackend::default());
        let executor = ActionExecutor::new(backend);
        executor.move_offset(10, 20, false).unwrap();
        let (x, y) = executor.get_position().unwrap();
        assert_eq!(x, 10);
        assert_eq!(y, 20);
    }

    #[test]
    fn test_type_text_at() {
        let backend = Box::new(MockBackend::default());
        let executor = ActionExecutor::new(backend);
        let result = executor.type_text_at(100, 200, "hello").unwrap();
        assert!(result.success);
    }

    #[test]
    fn mouse_button_parses_osworld_names() {
        assert_eq!(MouseButton::parse("left"), Some(MouseButton::Left));
        assert_eq!(MouseButton::parse(" MIDDLE "), Some(MouseButton::Middle));
        assert_eq!(MouseButton::parse("right"), Some(MouseButton::Right));
        assert_eq!(MouseButton::parse("scroll"), None);
        assert_eq!(MouseButton::Middle.as_str(), "middle");
    }

    #[test]
    fn pixel_scroll_converts_to_whole_lines() {
        assert_eq!(scroll_lines_for_pixels(0), 0);
        assert_eq!(scroll_lines_for_pixels(100), 1);
        assert_eq!(scroll_lines_for_pixels(-100), -1);
        assert_eq!(scroll_lines_for_pixels(350), 4);
        assert_eq!(scroll_lines_for_pixels(-249), -2);
        // A non-zero pixel delta must never collapse to a no-op scroll.
        assert_eq!(scroll_lines_for_pixels(1), 1);
        assert_eq!(scroll_lines_for_pixels(-1), -1);
    }

    #[test]
    fn click_button_at_covers_all_three_buttons() {
        let backend = Box::new(MockBackend::default());
        let executor = ActionExecutor::new(backend);
        executor
            .click_button_at(MouseButton::Left, 10, 20, 1, false)
            .unwrap();
        executor
            .click_button_at(MouseButton::Left, 10, 20, 2, false)
            .unwrap();
        executor
            .click_button_at(MouseButton::Right, 10, 20, 1, false)
            .unwrap();
        executor
            .click_button_at(MouseButton::Middle, 10, 20, 1, false)
            .unwrap();
        // Unsupported counts are rejected, never collapsed to one click.
        assert!(executor
            .click_button_at(MouseButton::Middle, 10, 20, 2, false)
            .is_err());
        assert!(executor
            .click_button_at(MouseButton::Left, 10, 20, 3, false)
            .is_err());
    }

    #[test]
    fn click_button_here_covers_all_three_buttons() {
        let backend = Box::new(MockBackend::default());
        let executor = ActionExecutor::new(backend);
        executor.click_button_here(MouseButton::Left, 1).unwrap();
        executor.click_button_here(MouseButton::Left, 2).unwrap();
        executor.click_button_here(MouseButton::Right, 1).unwrap();
        executor.click_button_here(MouseButton::Middle, 1).unwrap();
        assert!(executor.click_button_here(MouseButton::Right, 2).is_err());
    }

    #[test]
    fn scroll_at_current_pixels_drives_both_axes() {
        let backend = Box::new(MockBackend::default());
        let executor = ActionExecutor::new(backend);
        executor.scroll_at_current_pixels(200, -300).unwrap();
        executor.scroll_at_current_pixels(0, 0).unwrap();
    }

    #[test]
    fn drag_from_current_uses_the_live_pointer_as_start() {
        let backend = Box::new(MockBackend::default());
        let executor = ActionExecutor::new(backend);
        executor.move_to(5, 6, false).unwrap();
        executor.drag_left_from_current_to(50, 60, false).unwrap();
        assert_eq!(executor.get_position().unwrap(), (50, 60));
        // Same start and end is still an error: nothing to drag.
        assert!(executor.drag_left_from_current_to(50, 60, false).is_err());
    }
}
