//! L1 control abstraction: the OSWorld `computer_13` action set plus observation.
//!
//! [`LocalControl`] drives this host directly. A remote implementation (OSWorld
//! VM over HTTP) implements the same trait, so L2 and the tool layer never
//! branch on which backend is in use.
//!
//! # Observation contract
//!
//! `screenshot()` and `screen_size()` must describe the **same** pixel space.
//! Local capture reads physical pixels and resamples to the logical resolution,
//! so both report logical pixels. A backend that breaks this makes every
//! locator result land at a scaled offset.
//!
//! # Coordinates
//!
//! All coordinates in this trait are **logical screen pixels**, matching what
//! the model sees in the screenshot it is given.

use anyhow::Result;

use super::input::actions::MouseButton;

pub mod local;
pub mod remote;

pub use local::LocalControl;
pub use remote::{RemoteControl, RemoteTransport};

/// The OSWorld `computer_13` actions plus screen observation.
pub trait ComputerControl: Send + Sync {
    /// `MOVE_TO` — move the pointer to `(x, y)`.
    fn move_to(&self, x: i32, y: i32) -> Result<()>;

    /// `CLICK` — with coordinates, or at the current pointer when both are `None`.
    ///
    /// `clicks` is 1 or 2. Implementations must reject counts they cannot
    /// express rather than silently collapsing them to 1.
    fn click(
        &self,
        button: MouseButton,
        x: Option<i32>,
        y: Option<i32>,
        clicks: u32,
    ) -> Result<()>;

    /// `MOUSE_DOWN` — press a button at the current pointer.
    fn mouse_down(&self, button: MouseButton) -> Result<()>;

    /// `MOUSE_UP` — release a button at the current pointer.
    fn mouse_up(&self, button: MouseButton) -> Result<()>;

    /// `RIGHT_CLICK` at `(x, y)`.
    fn right_click(&self, x: i32, y: i32) -> Result<()>;

    /// `DOUBLE_CLICK` at `(x, y)`.
    fn double_click(&self, x: i32, y: i32) -> Result<()>;

    /// `DRAG_TO` — drag from the current pointer to `(x, y)`.
    fn drag_to(&self, x: i32, y: i32) -> Result<()>;

    /// `SCROLL` — horizontal `dx`, vertical `dy`.
    fn scroll(&self, dx: i32, dy: i32) -> Result<()>;

    /// `TYPING` — type `text` into the focused widget.
    fn typing(&self, text: &str) -> Result<()>;

    /// `PRESS` — press and release one key.
    fn press(&self, key: &str) -> Result<()>;

    /// `KEY_DOWN` — hold a key.
    fn key_down(&self, key: &str) -> Result<()>;

    /// `KEY_UP` — release a key.
    fn key_up(&self, key: &str) -> Result<()>;

    /// `HOTKEY` — press `keys` in order, release in reverse.
    fn hotkey(&self, keys: &[String]) -> Result<()>;

    /// `WAIT` — terminal signal; sleeps for `ms` milliseconds.
    fn wait(&self, ms: u64) -> Result<()>;

    /// Current screen as JPEG bytes at logical-pixel resolution.
    fn screenshot(&self) -> Result<Vec<u8>>;

    /// Screen size in logical pixels; matches [`screenshot`](Self::screenshot).
    fn screen_size(&self) -> Result<(u32, u32)>;

    /// Pointer position in logical screen pixels (diagnostics only).
    fn cursor_position(&self) -> Result<(i32, i32)>;
}
