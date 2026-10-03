//! Headless replacement for [`super::screen`] when the `computer` feature is off.
//!
//! Keeps the same public surface (types, constants, pure image/base64 helpers) so the
//! rest of the crate — chat_service, platform/app_access, computer pipeline — still
//! compiles without the `xcap`/`enigo` display-server stack. Every capture / monitor
//! discovery entry point returns an error instead of touching the OS.

use anyhow::{anyhow, Result};
use image::DynamicImage;

use crate::media::jpeg_vision::prepare_screenshot_jpeg;
use crate::models::ComputerMonitor;

/// JPEG quality for raw screen capture (mirrors the real module).
pub const SCREENSHOT_JPEG_QUALITY: u8 = 88;

/// One capture: JPEG for annotate, geometry, and global pointer at shot time.
#[derive(Debug, Clone)]
pub struct ScreenshotPacket {
    pub jpeg: Vec<u8>,
    pub monitor: MonitorInfo,
    pub capture_px: (u32, u32),
    pub global_pointer: (i32, i32),
    /// Text-focus hint in **global screen** coordinates, if available.
    pub global_caret: Option<(i32, i32)>,
}

/// Information about a monitor/screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorInfo {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

impl MonitorInfo {
    /// Create a new MonitorInfo.
    pub fn new(left: i32, top: i32, width: i32, height: i32) -> Self {
        Self {
            left,
            top,
            width,
            height,
        }
    }

    /// Check if a point (x, y) is within this monitor.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.left + self.width && y >= self.top && y < self.top + self.height
    }

    pub fn stable_id(&self) -> String {
        format!("{},{},{},{}", self.left, self.top, self.width, self.height)
    }
}

/// Prefix for OS-stable monitor ids (same value as the GUI module).
pub const XCAP_MONITOR_ID_PREFIX: &str = "xcap:";

/// How a stored monitor id was resolved for capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorResolveKind {
    /// `xcap:{id}` exact match.
    XcapId,
    /// Legacy `{left},{top},{width},{height}` exact match.
    LegacyBoundsExact,
    /// Legacy id matched same `(left, top)` after resolution / scaling change.
    LegacyBoundsOrigin,
    /// Auto mode: monitor under cursor.
    Cursor,
    /// Stale id: fell back to primary display.
    FallbackPrimary,
    /// Stale id: fell back to monitor under cursor.
    FallbackCursor,
}

/// Result of [`screenshot_for_selection`]: capture bytes plus optional refreshed monitor id.
#[derive(Debug, Clone)]
pub struct MonitorCapturePlan {
    pub packet: ScreenshotPacket,
    /// When set, persist this id (replaces a stale legacy or fuzzy-matched id).
    pub refreshed_monitor_id: Option<String>,
    pub resolve_kind: MonitorResolveKind,
}

fn headless_error(what: &str) -> anyhow::Error {
    anyhow!("computer feature disabled: {what} requires a display server (rebuild with `--features computer`)")
}

/// List all monitors for UI selection — unavailable without the `computer` feature.
pub fn list_monitors() -> Result<Vec<ComputerMonitor>> {
    Err(headless_error("monitor discovery"))
}

/// Stable id of the primary display — unavailable without the `computer` feature.
pub fn primary_monitor_id() -> Result<String> {
    Err(headless_error("monitor discovery"))
}

/// Map a global screen point to the containing monitor id — unavailable headless.
pub fn monitor_id_at_global_point(_x: i32, _y: i32) -> Result<String> {
    Err(headless_error("monitor discovery"))
}

/// Capture using a stored monitor id — unavailable without the `computer` feature.
pub fn screenshot_for_selection(_stored_id: Option<&str>) -> Result<MonitorCapturePlan> {
    Err(headless_error("screen capture"))
}

/// Capture a specific monitor by stored id — unavailable headless.
pub fn screenshot_monitor_by_id(_monitor_id: &str) -> Result<ScreenshotPacket> {
    Err(headless_error("screen capture"))
}

/// Capture the monitor containing the cursor — unavailable without the `computer` feature.
pub fn screenshot_current_monitor() -> Result<ScreenshotPacket> {
    Err(headless_error("screen capture"))
}

/// Encode logical RGBA to JPEG with vision byte cap (pure; no display needed).
pub fn rgba_to_jpeg_bytes(rgba: image::RgbaImage, _quality: u8) -> Result<Vec<u8>> {
    prepare_screenshot_jpeg(DynamicImage::ImageRgba8(rgba), "screenshot")
}

/// MIME type for OpenAI-style `data:` URLs from encoded image bytes.
pub fn image_data_url_mime(bytes: &[u8]) -> &'static str {
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        "image/jpeg"
    } else if bytes.len() >= 8 && bytes[0..8] == [137, 80, 78, 71, 13, 10, 26, 10] {
        "image/png"
    } else {
        "image/jpeg"
    }
}

/// MIME type after base64 decode (falls back to PNG on decode error).
pub fn image_data_url_mime_from_base64(b64: &str) -> &'static str {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD
        .decode(b64)
        .map(|raw| image_data_url_mime(&raw))
        .unwrap_or("image/png")
}

/// Human-readable byte size for observability logs (e.g. `842.3KB`, `1.2MB`).
pub fn format_data_size_bytes(len: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * KB;
    if len as f64 >= MB {
        format!("{:.1}MB", len as f64 / MB)
    } else if len >= 1024 {
        format!("{:.1}KB", len as f64 / KB)
    } else {
        format!("{len}B")
    }
}

pub fn encode_image_to_base64(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_info_contains_matches_gui_module() {
        let monitor = MonitorInfo::new(0, 0, 1920, 1080);
        assert!(monitor.contains(100, 100));
        assert!(!monitor.contains(1920, 1080));
    }

    #[test]
    fn capture_paths_error_without_computer_feature() {
        assert!(screenshot_current_monitor().is_err());
        assert!(list_monitors().is_err());
        assert!(primary_monitor_id().is_err());
    }
}
