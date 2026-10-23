//! Clipboard plain-text access, dispatched to the process main thread.
//!
//! On macOS the clipboard is `NSPasteboard`, and AppKit only allows it to be touched from the main
//! thread. `arboard` declares `Clipboard` as `Send + Sync` and reaches the pasteboard through a bare
//! `msg_send![NSPasteboard::class(), generalPasteboard]`, so nothing stops a Tokio worker from using
//! it — and when that happens AppKit hands work to `com.apple.main-thread` from the wrong queue,
//! libdispatch trips its assertion, and the process aborts:
//!
//! ```text
//! BUG IN CLIENT OF LIBDISPATCH: Assertion failed:
//! Block was expected to execute on queue [com.apple.main-thread]
//! ```
//!
//! That is a native abort: `catch_unwind` cannot intercept it and no error is logged. Routing every
//! pasteboard touch through [`crate::platform::run_on_main_thread`] is the only fix, so this module
//! is the single place in the crate that may call `arboard` directly.
//!
//! Nested calls are free: [`crate::platform::run_on_main_thread`] runs the closure inline when the
//! caller already is on the main thread.

use anyhow::Result;

/// Read the clipboard as plain text.
#[cfg(feature = "computer")]
pub fn read_text() -> Result<String> {
    crate::platform::run_on_main_thread(|| {
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| anyhow::anyhow!("Clipboard unavailable: {e}"))?;
        clipboard
            .get_text()
            .map_err(|e| anyhow::anyhow!("read_clipboard failed: {e}"))
    })?
}

/// Write plain text to the clipboard.
#[cfg(feature = "computer")]
pub fn write_text(text: &str) -> Result<()> {
    let owned = text.to_string();
    crate::platform::run_on_main_thread(move || {
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| anyhow::anyhow!("Clipboard unavailable: {e}"))?;
        clipboard
            .set_text(owned)
            .map_err(|e| anyhow::anyhow!("write_clipboard failed: {e}"))
    })?
}

/// Replace the clipboard content and return what was there before.
///
/// Read and write happen in one main-thread hop: a save/restore pair around a paste must not pay
/// two dispatch round-trips, and both halves need the same guarantee.
///
/// `Ok(None)` means there was nothing to save — an empty clipboard, or one holding non-text content
/// (an image, copied files). That is not an error.
#[cfg(feature = "computer")]
pub fn replace_text(text: &str) -> Result<Option<String>> {
    let owned = text.to_string();
    crate::platform::run_on_main_thread(move || -> Result<Option<String>> {
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| anyhow::anyhow!("Clipboard unavailable: {e}"))?;
        let saved = clipboard.get_text().ok();
        clipboard
            .set_text(owned)
            .map_err(|e| anyhow::anyhow!("write_clipboard failed: {e}"))?;
        Ok(saved)
    })?
}

/// Read the clipboard as plain text (headless builds return an error).
#[cfg(not(feature = "computer"))]
pub fn read_text() -> Result<String> {
    Err(anyhow::anyhow!(
        "computer feature disabled: clipboard backend unavailable"
    ))
}

/// Write plain text to the clipboard (headless builds return an error).
#[cfg(not(feature = "computer"))]
pub fn write_text(_text: &str) -> Result<()> {
    Err(anyhow::anyhow!(
        "computer feature disabled: clipboard backend unavailable"
    ))
}

/// Replace the clipboard content and return what was there before (headless builds return an error).
#[cfg(not(feature = "computer"))]
pub fn replace_text(_text: &str) -> Result<Option<String>> {
    Err(anyhow::anyhow!(
        "computer feature disabled: clipboard backend unavailable"
    ))
}
