//! Window chrome toggles for computer compact dock bar mode.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::{LogicalSize, Position, Size, WebviewWindow, window::Color};
#[cfg(not(target_os = "macos"))]
use tauri::LogicalPosition;
#[cfg(not(target_os = "linux"))]
use tauri::PhysicalPosition;

const TRANSPARENT: Color = Color(0, 0, 0, 0);
const RESTORE_MIN_WIDTH: f64 = 960.0;
const RESTORE_MIN_HEIGHT: f64 = 640.0;

static COMPUTER_COMPACT_CHROME: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
struct SavedCompactWindowState {
    logical_inner_width: f64,
    logical_inner_height: f64,
    logical_outer_x: f64,
    logical_outer_y: f64,
    maximized: bool,
}

static SAVED_COMPACT_WINDOW: Mutex<Option<SavedCompactWindowState>> = Mutex::new(None);

/// Whether the main window is in computer compact dock mode (skip traffic-light repair).
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn is_computer_compact_chrome_active() -> bool {
    COMPUTER_COMPACT_CHROME.load(Ordering::Relaxed)
}

#[tauri::command]
pub fn set_computer_compact_chrome(window: WebviewWindow, compact: bool) -> Result<(), String> {
    COMPUTER_COMPACT_CHROME.store(compact, Ordering::Relaxed);
    if compact {
        apply_compact_chrome(&window)?;
        log::debug!("computer compact chrome: compact mode applied");
    } else {
        restore_full_window_chrome(&window)?;
        log::debug!("computer compact chrome: full UI chrome restored");
    }
    Ok(())
}

/// Capture pre-compact geometry (logical inner + outer top-left) before compact chrome.
fn capture_window_geometry(window: &WebviewWindow) -> Result<SavedCompactWindowState, String> {
    let scale = window
        .scale_factor()
        .map_err(|e| format!("scale_factor: {e}"))?
        .max(1.0);
    let inner = window.inner_size().map_err(|e| format!("inner_size: {e}"))?;
    let pos = window
        .outer_position()
        .map_err(|e| format!("outer_position: {e}"))?;
    Ok(SavedCompactWindowState {
        logical_inner_width: inner.width as f64 / scale,
        logical_inner_height: inner.height as f64 / scale,
        logical_outer_x: pos.x as f64 / scale,
        logical_outer_y: pos.y as f64 / scale,
        maximized: false,
    })
}

/// Capture pre-compact geometry in Rust, then enter compact chrome (call before `place_computer_compact_window`).
#[tauri::command]
pub async fn begin_computer_compact_window(window: WebviewWindow) -> Result<(), String> {
    if SAVED_COMPACT_WINDOW.lock().unwrap().is_some() {
        log::warn!("begin_computer_compact_window: overwriting existing saved state");
    }

    let maximized = window.is_maximized().map_err(|e| format!("is_maximized: {e}"))?;
    let mut saved = if maximized {
        window
            .unmaximize()
            .map_err(|e| format!("unmaximize: {e}"))?;
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        capture_window_geometry(&window)?
    } else {
        capture_window_geometry(&window)?
    };
    saved.maximized = maximized;

    log::debug!(
        "begin_computer_compact_window: saved logical_inner={}x{} logical_pos=({},{}) maximized={}",
        saved.logical_inner_width,
        saved.logical_inner_height,
        saved.logical_outer_x,
        saved.logical_outer_y,
        saved.maximized
    );

    *SAVED_COMPACT_WINDOW.lock().unwrap() = Some(saved);
    COMPUTER_COMPACT_CHROME.store(true, Ordering::Relaxed);
    window
        .set_min_size(Some(Size::Logical(LogicalSize::new(280.0, 48.0))))
        .map_err(|e| format!("set_min_size: {e}"))?;
    apply_compact_chrome(&window)?;
    Ok(())
}

/// Restore pre-compact geometry and full window chrome (macOS: single coordinated path).
#[tauri::command]
pub async fn restore_computer_compact_window(window: WebviewWindow) -> Result<(), String> {
    let saved = SAVED_COMPACT_WINDOW
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| "restore_computer_compact_window: no saved window state".to_string())?;

    log::debug!(
        "restore_computer_compact_window: target logical_inner={}x{} logical_pos=({},{}) maximized={}",
        saved.logical_inner_width,
        saved.logical_inner_height,
        saved.logical_outer_x,
        saved.logical_outer_y,
        saved.maximized
    );

    COMPUTER_COMPACT_CHROME.store(false, Ordering::Relaxed);

    #[cfg(target_os = "macos")]
    restore_compact_window_macos(&window, &saved).await?;

    #[cfg(not(target_os = "macos"))]
    restore_compact_window_other(&window, &saved).await?;

    log::debug!(
        "restore_computer_compact_window: done outer={:?} inner={:?}",
        window.outer_size(),
        window.inner_size()
    );
    Ok(())
}

#[cfg(not(target_os = "macos"))]
async fn apply_saved_geometry(
    window: &WebviewWindow,
    saved: &SavedCompactWindowState,
) -> Result<(), String> {
    if saved.maximized {
        window
            .maximize()
            .map_err(|e| format!("maximize: {e}"))?;
        return Ok(());
    }

    window
        .set_size(Size::Logical(LogicalSize::new(
            saved.logical_inner_width,
            saved.logical_inner_height,
        )))
        .map_err(|e| format!("set_size: {e}"))?;
    tokio::time::sleep(std::time::Duration::from_millis(60)).await;
    window
        .set_position(Position::Logical(LogicalPosition::new(
            saved.logical_outer_x,
            saved.logical_outer_y,
        )))
        .map_err(|e| format!("set_position: {e}"))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn apply_saved_geometry_on_main_thread(
    window: &WebviewWindow,
    saved: &SavedCompactWindowState,
) -> Result<(), String> {
    if saved.maximized {
        return Ok(());
    }
    let win = window.clone();
    let saved = saved.clone();
    window
        .run_on_main_thread(move || {
            if let Ok(ns_window) = win.ns_window() {
                crate::macos_traffic_lights::set_window_geometry(
                    ns_window,
                    saved.logical_inner_width,
                    saved.logical_inner_height,
                    saved.logical_outer_x,
                    saved.logical_outer_y,
                );
            }
        })
        .map_err(|e| format!("run_on_main_thread(geometry): {e}"))
}

#[cfg(target_os = "macos")]
async fn restore_compact_window_macos(
    window: &WebviewWindow,
    saved: &SavedCompactWindowState,
) -> Result<(), String> {
    restore_full_window_chrome(window)?;
    set_window_and_webview_background(window, None)?;
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;

    window
        .set_min_size(Some(Size::Logical(LogicalSize::new(
            RESTORE_MIN_WIDTH,
            RESTORE_MIN_HEIGHT,
        ))))
        .map_err(|e| format!("set_min_size: {e}"))?;

    apply_saved_geometry_on_main_thread(window, saved)?;
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    crate::reapply_macos_window_chrome(window);
    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
    crate::repair_macos_overlay_chrome(window, "compact-restore");

    if saved.maximized {
        window
            .maximize()
            .map_err(|e| format!("maximize: {e}"))?;
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        crate::repair_macos_overlay_chrome(window, "compact-restore-maximized");
    } else {
        // Chrome repair can reset frame; re-apply saved geometry on the main thread.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        apply_saved_geometry_on_main_thread(window, saved)?;
    }

    let win_drag = window.clone();
    let _ = window.run_on_main_thread(move || {
        if let Ok(ns_window) = win_drag.ns_window() {
            crate::macos_traffic_lights::enable_window_dragging(ns_window);
        }
    });

    let _ = window.set_focus();
    Ok(())
}

#[cfg(not(target_os = "macos"))]
async fn restore_compact_window_other(
    window: &WebviewWindow,
    saved: &SavedCompactWindowState,
) -> Result<(), String> {
    window
        .set_min_size(Some(Size::Logical(LogicalSize::new(
            RESTORE_MIN_WIDTH,
            RESTORE_MIN_HEIGHT,
        ))))
        .map_err(|e| format!("set_min_size: {e}"))?;
    apply_saved_geometry(window, saved).await?;
    restore_full_window_chrome(window)?;
    set_window_and_webview_background(window, None)?;
    Ok(())
}

fn set_window_and_webview_background(
    window: &WebviewWindow,
    color: Option<Color>,
) -> Result<(), String> {
    window
        .set_background_color(color)
        .map_err(|e| format!("set_background_color: {e}"))
}

fn apply_compact_chrome(window: &WebviewWindow) -> Result<(), String> {
    if let Err(e) = set_window_and_webview_background(window, Some(TRANSPARENT)) {
        log::warn!("computer compact chrome: transparent background failed: {e}");
    }

    window
        .set_decorations(false)
        .map_err(|e| format!("set_decorations(false): {e}"))?;

    #[cfg(target_os = "macos")]
    apply_macos_compact_chrome(window)?;

    #[cfg(target_os = "windows")]
    apply_windows_compact_transparency(window)?;

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    log::info!("computer compact chrome: frameless (no OS title bar on Linux)");

    Ok(())
}

/// WebView2 only treats alpha=0 as transparent; resize after compact placement resets it on Windows.
#[cfg(target_os = "windows")]
fn apply_windows_compact_transparency(window: &WebviewWindow) -> Result<(), String> {
    window
        .set_background_color(Some(TRANSPARENT))
        .map_err(|e| format!("windows compact transparency: {e}"))
}

#[cfg(target_os = "windows")]
fn schedule_windows_compact_transparency_reapply(window: &WebviewWindow) {
    let win = window.clone();
    tauri::async_runtime::spawn(async move {
        for delay_ms in [50_u64, 150, 300] {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            if !is_computer_compact_chrome_active() {
                return;
            }
            if let Err(e) = apply_windows_compact_transparency(&win) {
                log::warn!("windows compact transparency reapply: {e}");
            }
        }
    });
}

#[cfg(target_os = "macos")]
fn apply_macos_compact_chrome(window: &WebviewWindow) -> Result<(), String> {
    let win = window.clone();
    window
        .run_on_main_thread(move || {
            if let Ok(ns_window) = win.ns_window() {
                crate::macos_traffic_lights::set_traffic_lights_visible(ns_window, false);
                crate::macos_traffic_lights::set_compact_surface(ns_window, true);
            }
        })
        .map_err(|e| format!("run_on_main_thread: {e}"))?;
    Ok(())
}

/// Windows / Linux: no native title bar — custom min/max/close in `AppShell`.
#[cfg(not(target_os = "macos"))]
pub fn reapply_frameless_window_chrome(window: &WebviewWindow) -> Result<(), String> {
    window
        .set_decorations(false)
        .map_err(|e| format!("set_decorations(false): {e}"))?;
    log::info!("frameless window chrome: OS title bar disabled (custom window controls)");
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn configure_frameless_window_chrome(window: &WebviewWindow) {
    if let Err(e) = reapply_frameless_window_chrome(window) {
        log::warn!("frameless window chrome init failed: {e}");
    }
}

fn reapply_window_chrome_internal(window: &WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::reapply_macos_window_chrome(window);
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        reapply_frameless_window_chrome(window)
    }
}

/// Re-apply platform window chrome (e.g. after resize/maximize from compact dock).
#[tauri::command]
pub fn reapply_window_chrome(window: WebviewWindow) -> Result<(), String> {
    reapply_window_chrome_internal(&window)
}

/// Resize and move the main window to the current monitor's bottom-right work area.
#[tauri::command]
pub async fn place_computer_compact_window(
    window: WebviewWindow,
    width: f64,
    height: f64,
    margin: f64,
) -> Result<(), String> {
    if window.is_maximized().map_err(|e| format!("is_maximized: {e}"))? {
        window.unmaximize().map_err(|e| format!("unmaximize: {e}"))?;
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        }
    }

    window
        .set_min_size(Some(Size::Logical(LogicalSize::new(280.0, 48.0))))
        .map_err(|e| format!("set_min_size: {e}"))?;

    window
        .set_size(Size::Logical(LogicalSize::new(width, height)))
        .map_err(|e| format!("set_size: {e}"))?;

    #[cfg(target_os = "linux")]
    {
        // GTK move/resize is most reliable with logical coords after the size change settles.
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        let scale = window.scale_factor().map_err(|e| format!("scale_factor: {e}"))?;
        let (x, y) = linux_compact_logical_position(&window, width, height, margin, scale)?;
        apply_logical_position(&window, x, y)?;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        apply_logical_position(&window, x, y)?;
        log::debug!(
            "computer compact window placed (linux): logical={width}x{height} at ({x},{y}) scale={scale}"
        );
    }

    #[cfg(not(target_os = "linux"))]
    {
        let monitor = window
            .current_monitor()
            .map_err(|e| format!("current_monitor: {e}"))?
            .ok_or_else(|| "current_monitor returned None".to_string())?;

        let scale = monitor.scale_factor();
        let work = monitor.work_area();
        let physical_w = (width * scale).round() as i32;
        let physical_h = (height * scale).round() as i32;
        let physical_margin = (margin * scale).round() as i32;

        let x = work.position.x + work.size.width as i32 - physical_w - physical_margin;
        let y = work.position.y + work.size.height as i32 - physical_h - physical_margin;

        window
            .set_position(Position::Physical(PhysicalPosition::new(x, y)))
            .map_err(|e| format!("set_position: {e}"))?;

        log::debug!(
            "computer compact window placed: logical={width}x{height} physical=({x},{y}) scale={scale}"
        );
    }

    // Resize can reset macOS traffic-light visibility; re-hide after placement.
    #[cfg(target_os = "macos")]
    if is_computer_compact_chrome_active() {
        apply_macos_compact_chrome(&window)?;
        schedule_macos_compact_chrome_reapply(&window);
    }

    #[cfg(target_os = "windows")]
    if is_computer_compact_chrome_active() {
        apply_windows_compact_transparency(&window)?;
        schedule_windows_compact_transparency_reapply(&window);
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn apply_logical_position(window: &WebviewWindow, x: f64, y: f64) -> Result<(), String> {
    window
        .set_position(Position::Logical(LogicalPosition::new(x, y)))
        .map_err(|e| format!("set_position(logical): {e}"))
}

/// Bottom-right placement on Linux using xcap monitor bounds (same source as Computer agent).
#[cfg(target_os = "linux")]
fn linux_compact_logical_position(
    window: &WebviewWindow,
    width: f64,
    height: f64,
    margin: f64,
    scale: f64,
) -> Result<(f64, f64), String> {
    let (left, top, mon_w, mon_h) = linux_monitor_bounds_for_window(window)?;
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let left_log = left as f64 / scale;
    let top_log = top as f64 / scale;
    let w_log = mon_w as f64 / scale;
    let h_log = mon_h as f64 / scale;
    let x = (left_log + w_log - width - margin).round();
    let y = (top_log + h_log - height - margin).round();
    Ok((x, y))
}

#[cfg(target_os = "linux")]
fn linux_monitor_bounds_for_window(window: &WebviewWindow) -> Result<(i32, i32, i32, i32), String> {
    use pointer_core::agents::computer::vision::screen;

    let monitors = screen::list_monitors().map_err(|e| format!("list_monitors: {e}"))?;
    if monitors.is_empty() {
        return Err("list_monitors: no displays".into());
    }

    let pos = window.outer_position().map_err(|e| format!("outer_position: {e}"))?;
    let size = window.outer_size().map_err(|e| format!("outer_size: {e}"))?;
    let cx = pos.x + size.width as i32 / 2;
    let cy = pos.y + size.height as i32 / 2;

    for m in &monitors {
        if cx >= m.left && cx < m.left + m.width && cy >= m.top && cy < m.top + m.height {
            log::debug!(
                "linux compact placement: window center ({cx},{cy}) on monitor {} ({}x{} @ {},{})",
                m.id,
                m.width,
                m.height,
                m.left,
                m.top
            );
            return Ok((m.left, m.top, m.width, m.height));
        }
    }

    // Wayland often reports outer_position (0,0); fall back to primary display.
    let fallback = monitors
        .iter()
        .find(|m| m.is_primary)
        .or(monitors.first())
        .expect("monitors non-empty");
    log::warn!(
        "linux compact placement: window center ({cx},{cy}) not on any monitor; using primary {} ({}x{} @ {},{})",
        fallback.id,
        fallback.width,
        fallback.height,
        fallback.left,
        fallback.top
    );
    Ok((fallback.left, fallback.top, fallback.width, fallback.height))
}

#[cfg(target_os = "macos")]
fn schedule_macos_compact_chrome_reapply(window: &WebviewWindow) {
    let win = window.clone();
    tauri::async_runtime::spawn(async move {
        for delay_ms in [50_u64, 200, 500] {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            if !is_computer_compact_chrome_active() {
                return;
            }
            if let Err(e) = apply_macos_compact_chrome(&win) {
                log::warn!("computer compact chrome: delayed reapply failed: {e}");
            }
        }
    });
}

fn restore_full_window_chrome(window: &WebviewWindow) -> Result<(), String> {
    reapply_window_chrome_internal(window)?;
    // Frontend sets theme bg via setBackgroundColor(hsl); None clears transparent override.
    set_window_and_webview_background(window, None)
}
