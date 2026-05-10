use anyhow::{anyhow, Result};
use enigo::{Enigo, Mouse, Settings};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ExtendedColorType};
use std::io::Cursor;
use std::process::Command;
use std::time::Instant;
use xcap::Monitor;

/// Picks the display under a global screen point (§3.2.1 `MonitorSelector`).
///
/// Wraps `xcap` monitor discovery so capture and tests share one entry point.
pub struct MonitorSelector;

impl MonitorSelector {
    /// Returns the [`Monitor`] handle and logical [`MonitorInfo`] for the display containing `(x, y)`.
    pub fn at_global_point(x: i32, y: i32) -> Result<(Monitor, MonitorInfo)> {
        let monitor =
            Monitor::from_point(x, y).map_err(|e| anyhow!("no monitor at ({x}, {y}): {e}"))?;
        let info = monitor_info_from_xcap(&monitor)?;
        Ok((monitor, info))
    }
}

/// JPEG quality for raw screen capture (before annotate). PNG compression was ~1–3s on 1080p+; JPEG is much faster.
pub const SCREENSHOT_JPEG_QUALITY: u8 = 88;

/// One capture: JPEG for annotate, geometry, and global pointer at shot time (Python `screen_overlay` input).
#[derive(Debug, Clone)]
pub struct ScreenshotPacket {
    pub jpeg: Vec<u8>,
    pub monitor: MonitorInfo,
    pub capture_px: (u32, u32),
    pub global_pointer: (i32, i32),
    /// Text-focus hint in **global screen** coordinates (Python `focus_position.get_focus_position()`), if available.
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
        x >= self.left
            && x < self.left + self.width
            && y >= self.top
            && y < self.top + self.height
    }
}

/// Capture a screenshot of the monitor that contains the current mouse cursor.
///
/// Returns **JPEG** bytes (lossy, fast encode), monitor geometry in **global logical screen coordinates**
/// (matches `CGDisplayBounds` / enigo pointer space), and the **bitmap pixel size** of the encoded image.
///
/// The OS capture is often **physical** pixels (e.g. macOS Retina). This path **resamples to logical
/// size** (`MonitorInfo.width` × `height`) before JPEG encode so the model and annotation boxes share
/// the same coordinate space as synthetic clicks—no separate Retina scale factor.
///
/// # Platform notes
/// Uses the [`xcap`](https://crates.io/crates/xcap) library (macOS, Windows, Linux). Linux under
/// **Wayland** may be limited depending on compositor and permissions; **X11** is generally
/// supported. If the cursor position cannot be read (e.g. input backend unavailable), the
/// **primary display** is used and a center point is chosen so the correct monitor is still
/// selected.
///
/// # Errors
/// Returns an error if no display is available or capture/encoding fails.
pub fn screenshot_current_monitor() -> Result<ScreenshotPacket> {
    let t_total = Instant::now();

    let t = Instant::now();
    let (cx, cy) = cursor_position()
        .or_else(|e| {
            log::debug!("cursor position unavailable ({}), using primary monitor center", e);
            primary_monitor_center()
        })?;
    let global_pointer = (cx, cy);

    let (monitor, info) = MonitorSelector::at_global_point(cx, cy)?;
    let setup_ms = t.elapsed().as_secs_f64() * 1000.0;

    let t = Instant::now();
    let rgba_raw = monitor
        .capture_image()
        .map_err(|e| anyhow!("screen capture failed: {}", e))?;
    let capture_ms = t.elapsed().as_secs_f64() * 1000.0;

    let logical_w = info.width.max(1) as u32;
    let logical_h = info.height.max(1) as u32;
    let physical = (rgba_raw.width(), rgba_raw.height());

    let t = Instant::now();
    let rgba = if physical.0 == logical_w && physical.1 == logical_h {
        rgba_raw
    } else {
        log::debug!(
            "screenshot_current_monitor: resampling physical {}x{} -> logical {}x{} (pointer / overlay space)",
            physical.0,
            physical.1,
            logical_w,
            logical_h
        );
        image::imageops::resize(&rgba_raw, logical_w, logical_h, FilterType::Triangle)
    };
    let resample_ms = t.elapsed().as_secs_f64() * 1000.0;

    let capture_px = (rgba.width(), rgba.height());

    let t = Instant::now();
    let jpeg = rgba_to_jpeg(rgba, SCREENSHOT_JPEG_QUALITY)?;
    let encode_ms = t.elapsed().as_secs_f64() * 1000.0;

    let global_caret = try_global_focus_caret_hint();

    let size_note = if physical.0 != logical_w || physical.1 != logical_h {
        format!(", from {}x{} physical", physical.0, physical.1)
    } else {
        String::new()
    };
    log::info!(
        "screenshot_current_monitor: cursor+monitor {:.1}ms, xcap_capture {:.1}ms, resample {:.1}ms, jpeg_encode q{} {:.1}ms, total {:.1}ms ({}x{} px logical{})",
        setup_ms,
        capture_ms,
        resample_ms,
        SCREENSHOT_JPEG_QUALITY,
        encode_ms,
        t_total.elapsed().as_secs_f64() * 1000.0,
        capture_px.0,
        capture_px.1,
        size_note
    );
    Ok(ScreenshotPacket {
        jpeg,
        monitor: info,
        capture_px,
        global_pointer,
        global_caret,
    })
}

/// Encode logical RGBA to JPEG (overlay pipeline after pointer marks).
pub fn rgba_to_jpeg_bytes(rgba: image::RgbaImage, quality: u8) -> Result<Vec<u8>> {
    rgba_to_jpeg(rgba, quality)
}

/// Best-effort focused-control hint for drawing the I-beam overlay (Python `agents/computer/focus_position.py`).
fn try_global_focus_caret_hint() -> Option<(i32, i32)> {
    #[cfg(target_os = "macos")]
    {
        try_global_focus_caret_hint_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn try_global_focus_caret_hint_macos() -> Option<(i32, i32)> {
    const SCRIPT: &str = r#"
tell application "System Events"
    set frontApp to first application process whose frontmost is true
    try
        set fe to (first UI element of frontApp whose focused is true)
        set {x1, y1, x2, y2} to (get value of attribute "AXFrame" of fe)
        set x to x1 + 12
        set y to y1 + (y2 - y1) / 2
        return (round x) as text & "," & (round y) as text
    on error err
        return "Error: " & err
    end try
end tell
"#;
    let out = Command::new("osascript")
        .args(["-e", SCRIPT.trim()])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let s = s.trim();
    if s.is_empty() || s.starts_with("Error:") {
        return None;
    }
    let mut parts = s.split(',');
    let x: i32 = parts.next()?.trim().parse().ok()?;
    let y: i32 = parts.next()?.trim().parse().ok()?;
    Some((x, y))
}

fn cursor_position() -> Result<(i32, i32)> {
    let enigo = Enigo::new(&Settings::default())
        .map_err(|e| anyhow!("enigo init for cursor position: {:?}", e))?;
    enigo
        .location()
        .map_err(|e| anyhow!("cursor position: {:?}", e))
}

fn primary_monitor_center() -> Result<(i32, i32)> {
    let monitors = Monitor::all().map_err(|e| anyhow!("list monitors: {}", e))?;
    let m = monitors
        .iter()
        .find(|mon| mon.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .ok_or_else(|| anyhow!("no displays found"))?;
    let x = m.x().map_err(|e| anyhow!("monitor x: {}", e))?;
    let y = m.y().map_err(|e| anyhow!("monitor y: {}", e))?;
    let w = m.width().map_err(|e| anyhow!("monitor width: {}", e))?;
    let h = m.height().map_err(|e| anyhow!("monitor height: {}", e))?;
    Ok((x + w as i32 / 2, y + h as i32 / 2))
}

fn monitor_info_from_xcap(m: &Monitor) -> Result<MonitorInfo> {
    Ok(MonitorInfo::new(
        m.x().map_err(|e| anyhow!("{}", e))?,
        m.y().map_err(|e| anyhow!("{}", e))?,
        m.width().map_err(|e| anyhow!("{}", e))? as i32,
        m.height().map_err(|e| anyhow!("{}", e))? as i32,
    ))
}

fn rgba_to_jpeg(rgba: image::RgbaImage, quality: u8) -> Result<Vec<u8>> {
    let rgb = DynamicImage::ImageRgba8(rgba).into_rgb8();
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);
    let mut enc = JpegEncoder::new_with_quality(&mut cursor, quality);
    enc.encode(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        ExtendedColorType::Rgb8,
    )
    .map_err(|e| anyhow!("JPEG encode failed: {}", e))?;
    Ok(buf)
}

/// Encode raw image bytes to a base64 string.
///
/// # Arguments
/// * `bytes` - Raw image bytes (e.g., JPEG, PNG).
///
/// # Returns
/// Base64-encoded string.
pub fn encode_image_to_base64(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitor_info_contains() {
        let monitor = MonitorInfo::new(0, 0, 1920, 1080);
        assert!(monitor.contains(100, 100));
        assert!(monitor.contains(1919, 1079));
        assert!(!monitor.contains(1920, 1080));
        assert!(!monitor.contains(-1, -1));
    }

    #[test]
    fn test_encode_image_to_base64() {
        let data = b"fake_png_data";
        let encoded = encode_image_to_base64(data);
        assert!(!encoded.is_empty());
        // Verify it's valid base64
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let decoded = STANDARD.decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }
}
