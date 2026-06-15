use anyhow::{anyhow, Result};
#[cfg(not(target_os = "macos"))]
use enigo::{Enigo, Mouse, Settings};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ExtendedColorType};
use std::io::Cursor;
#[cfg(target_os = "macos")]
use std::process::Command;
use std::time::Instant;
use xcap::Monitor;

use crate::models::ComputerMonitor;

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

    pub fn stable_id(&self) -> String {
        format!("{},{},{},{}", self.left, self.top, self.width, self.height)
    }
}

/// Prefix for OS-stable monitor ids from [`xcap::Monitor::id`] (survives resolution / scaling changes).
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoredMonitorId {
    Xcap(u32),
    LegacyBounds,
}

/// Build the id shown in the monitor picker and stored on conversations.
pub fn monitor_list_id(m: &Monitor) -> Result<String> {
    let id = m.id().map_err(|e| anyhow!("monitor id: {}", e))?;
    Ok(format!("{XCAP_MONITOR_ID_PREFIX}{id}"))
}

fn parse_stored_monitor_id(stored_id: &str) -> StoredMonitorId {
    if let Some(rest) = stored_id.strip_prefix(XCAP_MONITOR_ID_PREFIX) {
        if let Ok(n) = rest.parse::<u32>() {
            return StoredMonitorId::Xcap(n);
        }
    }
    StoredMonitorId::LegacyBounds
}

fn legacy_origin_from_id(stored_id: &str) -> Option<(i32, i32)> {
    let mut parts = stored_id.split(',');
    let left: i32 = parts.next()?.parse().ok()?;
    let top: i32 = parts.next()?.parse().ok()?;
    // Require four comma-separated numbers so we do not treat `xcap:1` as legacy.
    let _w: i32 = parts.next()?.parse().ok()?;
    let _h: i32 = parts.next()?.parse().ok()?;
    Some((left, top))
}

fn monitor_matches_stored_id(m: &Monitor, info: &MonitorInfo, stored_id: &str) -> Option<MonitorResolveKind> {
    match parse_stored_monitor_id(stored_id) {
        StoredMonitorId::Xcap(n) => {
            let xid = m.id().ok()?;
            (xid == n).then_some(MonitorResolveKind::XcapId)
        }
        StoredMonitorId::LegacyBounds => {
            if info.stable_id() == stored_id {
                return Some(MonitorResolveKind::LegacyBoundsExact);
            }
            let (ol, ot) = legacy_origin_from_id(stored_id)?;
            (info.left == ol && info.top == ot).then_some(MonitorResolveKind::LegacyBoundsOrigin)
        }
    }
}

fn list_monitor_pairs() -> Result<Vec<(Monitor, MonitorInfo)>> {
    let monitors = Monitor::all().map_err(|e| anyhow!("list monitors: {}", e))?;
    let mut out = Vec::with_capacity(monitors.len());
    for m in monitors {
        let info = monitor_info_from_xcap(&m)?;
        out.push((m, info));
    }
    Ok(out)
}

fn find_monitor_by_stored_id(stored_id: &str) -> Result<(Monitor, MonitorInfo, MonitorResolveKind)> {
    for (m, info) in list_monitor_pairs()? {
        if let Some(kind) = monitor_matches_stored_id(&m, &info, stored_id) {
            return Ok((m, info, kind));
        }
    }
    Err(anyhow!("monitor not found for id={}", stored_id))
}

fn find_primary_monitor() -> Result<(Monitor, MonitorInfo)> {
    let pairs = list_monitor_pairs()?;
    let mut first: Option<(Monitor, MonitorInfo)> = None;
    for (m, info) in pairs {
        if m.is_primary().unwrap_or(false) {
            return Ok((m, info));
        }
        if first.is_none() {
            first = Some((m, info));
        }
    }
    first.ok_or_else(|| anyhow!("no displays found"))
}

/// List all monitors for UI selection (`xcap:{id}` ids).
pub fn list_monitors() -> Result<Vec<ComputerMonitor>> {
    let monitors = Monitor::all().map_err(|e| anyhow!("list monitors: {}", e))?;
    let mut out: Vec<ComputerMonitor> = Vec::with_capacity(monitors.len());
    for m in monitors {
        let info = monitor_info_from_xcap(&m)?;
        let is_primary = m.is_primary().unwrap_or(false);
        out.push(ComputerMonitor {
            id: monitor_list_id(&m)?,
            left: info.left,
            top: info.top,
            width: info.width,
            height: info.height,
            is_primary,
            work_area: None,
        });
    }
    Ok(out)
}

fn screenshot_from_monitor(
    monitor: &Monitor,
    info: MonitorInfo,
    global_pointer: (i32, i32),
    log_label: &str,
) -> Result<ScreenshotPacket> {
    let t_total = Instant::now();

    let t = Instant::now();
    let rgba_raw = capture_monitor_rgba(monitor)?;
    let capture_ms = t.elapsed().as_secs_f64() * 1000.0;

    let logical_w = info.width.max(1) as u32;
    let logical_h = info.height.max(1) as u32;
    let physical = (rgba_raw.width(), rgba_raw.height());

    let t = Instant::now();
    let rgba = resample_capture_to_logical(rgba_raw, logical_w, logical_h, log_label);
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
        "{log_label}: xcap_capture {:.1}ms, resample {:.1}ms, jpeg_encode q{} {:.1}ms, total {:.1}ms ({}x{} px logical{})",
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

fn global_pointer_for_capture() -> Result<(i32, i32)> {
    cursor_position().or_else(|e| {
        log::debug!("cursor position unavailable ({}), using primary monitor center", e);
        primary_monitor_center()
    })
}

/// Capture using a stored monitor id, with stale-id recovery.
///
/// `stored_id = None` follows the cursor. Legacy bounds ids match exactly or by `(left, top)` only.
/// When the id is missing entirely, falls back to primary then cursor and returns a refreshed `xcap:{id}`.
pub fn screenshot_for_selection(stored_id: Option<&str>) -> Result<MonitorCapturePlan> {
    let global_pointer = global_pointer_for_capture()?;

    match stored_id {
        None => {
            let (monitor, info) = MonitorSelector::at_global_point(global_pointer.0, global_pointer.1)?;
            let packet = screenshot_from_monitor(&monitor, info, global_pointer, "screenshot_current_monitor")?;
            Ok(MonitorCapturePlan {
                packet,
                refreshed_monitor_id: None,
                resolve_kind: MonitorResolveKind::Cursor,
            })
        }
        Some(id) => match find_monitor_by_stored_id(id) {
            Ok((monitor, info, kind)) => {
                let needs_refresh = !matches!(
                    kind,
                    MonitorResolveKind::XcapId | MonitorResolveKind::LegacyBoundsExact
                );
                let refreshed_monitor_id = needs_refresh.then(|| monitor_list_id(&monitor)).transpose()?;
                if let Some(ref new_id) = refreshed_monitor_id {
                    log::info!(
                        "screenshot_for_selection: refreshed monitor id {id} -> {new_id} ({kind:?})"
                    );
                }
                let packet =
                    screenshot_from_monitor(&monitor, info, global_pointer, "screenshot_monitor_by_id")?;
                Ok(MonitorCapturePlan {
                    packet,
                    refreshed_monitor_id,
                    resolve_kind: kind,
                })
            }
            Err(e) => {
                log::warn!(
                    "screenshot_for_selection: stored monitor id stale ({id}): {:#}; trying primary then cursor",
                    e
                );
                if let Ok((monitor, info)) = find_primary_monitor() {
                    let new_id = monitor_list_id(&monitor)?;
                    log::info!(
                        "screenshot_for_selection: fallback primary display, refreshed id {id} -> {new_id}"
                    );
                    let packet =
                        screenshot_from_monitor(&monitor, info, global_pointer, "screenshot_monitor_primary_fallback")?;
                    return Ok(MonitorCapturePlan {
                        packet,
                        refreshed_monitor_id: Some(new_id),
                        resolve_kind: MonitorResolveKind::FallbackPrimary,
                    });
                }
                let (monitor, info) = MonitorSelector::at_global_point(global_pointer.0, global_pointer.1)?;
                let new_id = monitor_list_id(&monitor)?;
                log::info!(
                    "screenshot_for_selection: fallback cursor display, refreshed id {id} -> {new_id}"
                );
                let packet =
                    screenshot_from_monitor(&monitor, info, global_pointer, "screenshot_monitor_cursor_fallback")?;
                Ok(MonitorCapturePlan {
                    packet,
                    refreshed_monitor_id: Some(new_id),
                    resolve_kind: MonitorResolveKind::FallbackCursor,
                })
            }
        },
    }
}

/// Capture a specific monitor (by stored id). Uses [`screenshot_for_selection`] (includes stale-id recovery).
pub fn screenshot_monitor_by_id(monitor_id: &str) -> Result<ScreenshotPacket> {
    Ok(screenshot_for_selection(Some(monitor_id))?.packet)
}

/// Capture a screenshot of the monitor that contains the current mouse cursor.
///
/// Returns **JPEG** bytes (lossy, fast encode), monitor geometry in **global logical screen coordinates**
/// (matches `CGDisplayBounds` and synthetic clicks; on macOS the cursor is read via Quartz `CGEvent`,
/// not enigo, so it matches `xcap::Monitor::from_point`), and the **bitmap pixel size** of the encoded image.
///
/// The OS capture is often **physical** pixels (e.g. macOS Retina). This path **resamples to logical
/// size** (`MonitorInfo.width` × `height`) before JPEG encode so the model and annotation boxes share
/// the same coordinate space as synthetic clicks—no separate Retina scale factor.
///
/// # Platform notes
/// Uses [`xcap`] (Windows WGC omits the hardware cursor). Synthetic pointer is drawn in [`screen_overlay`].
/// Linux under **Wayland**
/// may be limited depending on compositor and permissions; **X11** is generally supported. If the
/// cursor position cannot be read (e.g. input backend unavailable), the **primary display** is used.
///
/// # Errors
/// Returns an error if no display is available or capture/encoding fails.
pub fn screenshot_current_monitor() -> Result<ScreenshotPacket> {
    Ok(screenshot_for_selection(None)?.packet)
}

/// OS framebuffer capture for one monitor via xcap.
fn capture_monitor_rgba(monitor: &Monitor) -> Result<image::RgbaImage> {
    monitor
        .capture_image()
        .map_err(|e| anyhow!("screen capture failed: {e}"))
}

fn resample_capture_to_logical(
    rgba_raw: image::RgbaImage,
    logical_w: u32,
    logical_h: u32,
    log_label: &str,
) -> image::RgbaImage {
    let physical = (rgba_raw.width(), rgba_raw.height());
    if physical.0 == logical_w && physical.1 == logical_h {
        rgba_raw
    } else {
        log::debug!(
            "{log_label}: resampling physical {}x{} -> logical {}x{} (pointer / overlay space)",
            physical.0,
            physical.1,
            logical_w,
            logical_h
        );
        image::imageops::resize(&rgba_raw, logical_w, logical_h, FilterType::Triangle)
    }
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
    #[cfg(target_os = "macos")]
    {
        cursor_position_macos_cg()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let enigo = Enigo::new(&Settings::default())
            .map_err(|e| anyhow!("enigo init for cursor position: {:?}", e))?;
        enigo
            .location()
            .map_err(|e| anyhow!("cursor position: {:?}", e))
    }
}

#[cfg(target_os = "macos")]
fn cursor_position_macos_cg() -> Result<(i32, i32)> {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|()| anyhow!("CGEventSource::new(HIDSystemState) failed"))?;
    let event = CGEvent::new(source).map_err(|()| anyhow!("CGEvent::new failed"))?;
    let pt = event.location();
    Ok((pt.x as i32, pt.y as i32))
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

/// MIME type for OpenAI-style `data:` URLs from encoded image bytes.
pub fn image_data_url_mime(bytes: &[u8]) -> &'static str {
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        "image/jpeg"
    } else if bytes.len() >= 8 && &bytes[0..8] == [137, 80, 78, 71, 13, 10, 26, 10] {
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

    #[test]
    fn legacy_origin_parses_bounds_id() {
        assert_eq!(legacy_origin_from_id("0,0,1512,950"), Some((0, 0)));
        assert_eq!(legacy_origin_from_id("1512,0,1920,1080"), Some((1512, 0)));
        assert!(legacy_origin_from_id("xcap:1").is_none());
        assert!(legacy_origin_from_id("0,0").is_none());
    }

    #[test]
    fn parse_stored_monitor_id_recognizes_xcap_and_legacy() {
        assert_eq!(parse_stored_monitor_id("xcap:3"), StoredMonitorId::Xcap(3));
        assert_eq!(
            parse_stored_monitor_id("0,0,1512,950"),
            StoredMonitorId::LegacyBounds
        );
    }

    #[test]
    fn legacy_bounds_origin_matches_same_top_left() {
        let info = MonitorInfo::new(0, 0, 1728, 1117);
        assert_eq!(info.stable_id(), "0,0,1728,1117");
        assert_ne!(info.stable_id(), "0,0,1512,950");
        let (ol, ot) = legacy_origin_from_id("0,0,1512,950").unwrap();
        assert_eq!(info.left, ol);
        assert_eq!(info.top, ot);
    }
}
