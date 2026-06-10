//! Window chrome toggles for computer compact dock bar mode.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{WebviewWindow, window::Color};

const TRANSPARENT: Color = Color(0, 0, 0, 0);

static COMPUTER_COMPACT_CHROME: AtomicBool = AtomicBool::new(false);

/// Whether the main window is in computer compact dock mode (skip traffic-light repair).
#[cfg(target_os = "macos")]
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

fn set_window_and_webview_background(
    window: &WebviewWindow,
    color: Option<Color>,
) -> Result<(), String> {
    window
        .set_background_color(color)
        .map_err(|e| format!("set_background_color: {e}"))
}

fn apply_compact_chrome(window: &WebviewWindow) -> Result<(), String> {
    set_window_and_webview_background(window, Some(TRANSPARENT))?;

    window
        .set_decorations(false)
        .map_err(|e| format!("set_decorations(false): {e}"))?;

    #[cfg(target_os = "macos")]
    apply_macos_compact_chrome(window)?;

    #[cfg(not(target_os = "macos"))]
    log::info!("computer compact chrome: frameless (no OS title bar on Windows/Linux)");

    Ok(())
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

fn restore_full_window_chrome(window: &WebviewWindow) -> Result<(), String> {
    reapply_window_chrome_internal(window)?;
    // Frontend sets theme bg via setBackgroundColor(hsl); None clears transparent override.
    set_window_and_webview_background(window, None)
}
