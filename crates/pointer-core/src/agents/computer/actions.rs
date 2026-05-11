use anyhow::{anyhow, Result};
use std::time::Duration;

/// After an absolute `move_to`, wait briefly so the OS / target app can update hit-testing (hover,
/// window activation, text-field focus, etc.) before the next click or scroll.
#[inline]
fn settle_after_absolute_move() {
    std::thread::sleep(Duration::from_millis(45));
}

/// Mouse button for low-level press/release.
#[derive(Debug, Clone, Copy)]
pub enum MouseButton {
    Left,
    Right,
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

    /// Move the cursor to the specified screen coordinates.
    ///
    /// # Arguments
    /// * `x` - Screen X coordinate in pixels.
    /// * `y` - Screen Y coordinate in pixels.
    fn move_to(&self, x: i32, y: i32) -> Result<ActionResult>;

    /// Scroll the mouse wheel.
    ///
    /// # Arguments
    /// * `lines` - Number of lines to scroll (positive = up, negative = down).
    fn scroll(&self, lines: i32) -> Result<ActionResult>;

    /// Type the given text.
    ///
    /// # Arguments
    /// * `text` - The text to type.
    fn type_text(&self, text: &str) -> Result<ActionResult>;

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
        self.backend.click()
    }

    /// Double-click at the current cursor.
    pub fn double_click_here(&self) -> Result<ActionResult> {
        self.backend.double_click()
    }

    /// Right-click at the current cursor.
    pub fn right_click_here(&self) -> Result<ActionResult> {
        self.backend.right_click()
    }

    /// Drag with left button from pixel (x1,y1) to (x2,y2).
    pub fn drag_left_from_to(&self, x1: i32, y1: i32, x2: i32, y2: i32) -> Result<ActionResult> {
        if x1 == x2 && y1 == y2 {
            return Err(anyhow!("drag start and end must differ"));
        }
        self.backend.move_to(x1, y1)?;
        settle_after_absolute_move();
        self.backend.mouse_phase(MouseButton::Left, KeyPhase::Press)?;
        self.backend.move_to(x2, y2)?;
        self.backend.mouse_phase(MouseButton::Left, KeyPhase::Release)?;
        Ok(ActionResult::success("drag completed"))
    }

    /// Hold primary multi-select modifier (Cmd on macOS, Ctrl elsewhere), click each pixel, release.
    pub fn click_add_to_selection_batch(&self, positions: &[(i32, i32)]) -> Result<ActionResult> {
        if positions.is_empty() {
            return Ok(ActionResult::success("no positions"));
        }
        let meta = if cfg!(target_os = "macos") {
            "command"
        } else {
            "ctrl"
        };
        self.backend.key_phase(meta, KeyPhase::Press)?;
        for &(x, y) in positions {
            self.backend.move_to(x, y)?;
            settle_after_absolute_move();
            self.backend.click()?;
        }
        self.backend.key_phase(meta, KeyPhase::Release)?;
        Ok(ActionResult::success("multi-select clicks"))
    }

    /// Click first, then Shift+click second (range selection).
    pub fn click_range_shift(&self, first: (i32, i32), last: (i32, i32)) -> Result<ActionResult> {
        self.click_at(first.0, first.1)?;
        self.backend.key_phase("shift", KeyPhase::Press)?;
        self.click_at(last.0, last.1)?;
        self.backend.key_phase("shift", KeyPhase::Release)?;
        Ok(ActionResult::success("shift range click"))
    }

    /// Click at the given screen coordinates (already resolved to pixels).
    ///
    /// Used by the **coordinate-based** positioning path.
    ///
    /// # Arguments
    /// * `x` - Screen X coordinate in pixels.
    /// * `y` - Screen Y coordinate in pixels.
    pub fn click_at(&self, x: i32, y: i32) -> Result<ActionResult> {
        self.backend.move_to(x, y)?;
        settle_after_absolute_move();
        self.backend.click()
    }

    /// Click at an annotated element index.
    ///
    /// The caller must resolve index → (x, y) via `vision_state.resolve_index()` first.
    /// Used by the **index-based** positioning path.
    ///
    /// # Arguments
    /// * `x` - Screen X coordinate in pixels (resolved from index).
    /// * `y` - Screen Y coordinate in pixels (resolved from index).
    pub fn click_index(&self, x: i32, y: i32) -> Result<ActionResult> {
        self.click_at(x, y)
    }

    /// Double-click at the given screen coordinates.
    ///
    /// Used by the **coordinate-based** positioning path.
    pub fn double_click_at(&self, x: i32, y: i32) -> Result<ActionResult> {
        self.backend.move_to(x, y)?;
        settle_after_absolute_move();
        self.backend.double_click()
    }

    /// Double-click at an annotated element index.
    ///
    /// Used by the **index-based** positioning path.
    pub fn double_click_index(&self, x: i32, y: i32) -> Result<ActionResult> {
        self.double_click_at(x, y)
    }

    /// Right-click at the given screen coordinates.
    ///
    /// Used by the **coordinate-based** positioning path.
    pub fn right_click_at(&self, x: i32, y: i32) -> Result<ActionResult> {
        self.backend.move_to(x, y)?;
        settle_after_absolute_move();
        self.backend.right_click()
    }

    /// Right-click at an annotated element index.
    ///
    /// Used by the **index-based** positioning path.
    pub fn right_click_index(&self, x: i32, y: i32) -> Result<ActionResult> {
        self.right_click_at(x, y)
    }

    /// Move the cursor to the given screen coordinates without clicking.
    ///
    /// Used by the **coordinate-based** positioning path.
    pub fn hover_at(&self, x: i32, y: i32) -> Result<ActionResult> {
        self.backend.move_to(x, y)
    }

    /// Move the cursor to an annotated element index.
    ///
    /// Used by the **index-based** positioning path.
    pub fn hover_index(&self, x: i32, y: i32) -> Result<ActionResult> {
        self.hover_at(x, y)
    }

    /// Move the cursor by a relative offset.
    ///
    /// # Arguments
    /// * `dx` - Delta X in pixels.
    /// * `dy` - Delta Y in pixels.
    pub fn move_offset(&self, dx: i32, dy: i32) -> Result<ActionResult> {
        let (current_x, current_y) = self.backend.get_position()?;
        self.backend.move_to(current_x + dx, current_y + dy)
    }

    /// Scroll at the current cursor position.
    ///
    /// # Arguments
    /// * `lines` - Number of lines to scroll.
    pub fn scroll_at_current(&self, lines: i32) -> Result<ActionResult> {
        self.backend.scroll(lines)
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
        self.type_text_at_with_options(x, y, text, false, false)
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
    ) -> Result<ActionResult> {
        self.click_at(x, y)?;
        if clear_first {
            self.hotkey_select_all()?;
        }
        self.backend.type_text(text)?;
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
    ) -> Result<ActionResult> {
        if clear_first {
            self.hotkey_select_all()?;
        }
        self.backend.type_text(text)?;
        if auto_enter {
            self.hotkey(&["return"])?;
        }
        Ok(ActionResult::success("typed focused"))
    }

    /// Scroll at a specific location.
    ///
    /// Sequence: move → scroll.
    pub fn scroll_at(&self, x: i32, y: i32, lines: i32) -> Result<ActionResult> {
        self.backend.move_to(x, y)?;
        settle_after_absolute_move();
        self.backend.scroll(lines)
    }

    /// Move to a specific location without clicking.
    pub fn move_to(&self, x: i32, y: i32) -> Result<ActionResult> {
        self.backend.move_to(x, y)
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

        fn move_to(&self, x: i32, y: i32) -> Result<ActionResult> {
            self.record(format!("move_to({},{})", x, y));
            *self.position.lock().unwrap() = (x, y);
            Ok(ActionResult::success("moved"))
        }

        fn scroll(&self, lines: i32) -> Result<ActionResult> {
            self.record(format!("scroll({})", lines));
            Ok(ActionResult::success("scrolled"))
        }

        fn type_text(&self, text: &str) -> Result<ActionResult> {
            self.record(format!("type_text({})", text));
            Ok(ActionResult::success("typed"))
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
        let result = executor.click_at(100, 200).unwrap();
        assert!(result.success);
    }

    #[test]
    fn test_click_index() {
        let backend = Box::new(MockBackend::default());
        let executor = ActionExecutor::new(backend);
        let result = executor.click_index(100, 200).unwrap();
        assert!(result.success);
    }

    #[test]
    fn test_move_offset() {
        let backend = Box::new(MockBackend::default());
        let executor = ActionExecutor::new(backend);
        executor.move_offset(10, 20).unwrap();
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
}
