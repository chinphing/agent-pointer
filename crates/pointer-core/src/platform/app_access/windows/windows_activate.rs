//! Activate running Windows apps — visible, hidden, or tray-only (restore / relaunch).

use super::windows_recent::{self, find_running_pid_for_app, process_image_path_for_pid};
use anyhow::Result;
use std::thread;
use std::time::Duration;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};

const TRAY_RELAUNCH_SETTLE_MS: u64 = 800;

/// Try to bring a running instance to the foreground (visible, hidden, or tray-only).
pub fn try_activate_running(app: &str) -> Result<Option<windows::Win32::Foundation::HWND>> {
    let app_trim = app.trim();
    if app_trim.is_empty() {
        return Ok(None);
    }

    if let Some(hwnd) = find_visible_window_for_app(app_trim)? {
        log::info!("launch_app Windows activate: visible window for app={app_trim}");
        return Ok(Some(hwnd));
    }

    if let Some(pid) = find_running_pid_for_app(app_trim)? {
        if let Some(hwnd) = find_top_level_window_for_pid(pid, false) {
            log::info!(
                "launch_app Windows activate: hidden window for app={app_trim} pid={pid}"
            );
            return Ok(Some(hwnd));
        }
        if try_relaunch_running_instance(app_trim, pid)? {
            thread::sleep(Duration::from_millis(TRAY_RELAUNCH_SETTLE_MS));
            if let Some(hwnd) = find_visible_window_for_app(app_trim)? {
                log::info!(
                    "launch_app Windows activate: restored via relaunch app={app_trim} pid={pid}"
                );
                return Ok(Some(hwnd));
            }
            log::warn!(
                "launch_app Windows activate: relaunch did not produce a visible window app={app_trim} pid={pid}"
            );
        }
    }

    Ok(None)
}

fn find_visible_window_for_app(app: &str) -> Result<Option<HWND>> {
    find_window_for_app(app, true)
}

fn find_window_for_app(app: &str, visible_only: bool) -> Result<Option<HWND>> {
    let needle = app.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return Ok(None);
    }

    struct FindCtx {
        needle: String,
        visible_only: bool,
        best: Option<(HWND, i64)>,
    }

    let mut ctx = FindCtx {
        needle,
        visible_only,
        best: None,
    };

    unsafe extern "system" fn find_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
            GW_OWNER,
        };

        let ctx = unsafe { &mut *(lparam.0 as *mut FindCtx) };
        if hwnd.is_invalid() {
            return BOOL(1);
        }
        if unsafe { GetWindow(hwnd, GW_OWNER) }
            .ok()
            .is_some_and(|owner| !owner.is_invalid())
        {
            return BOOL(1);
        }
        if ctx.visible_only && !unsafe { IsWindowVisible(hwnd).as_bool() } {
            return BOOL(1);
        }

        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == 0 {
            return BOOL(1);
        }

        let mut buf = [0u16; 512];
        let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
        let title = if len > 0 {
            String::from_utf16_lossy(&buf[..len as usize]).to_ascii_lowercase()
        } else {
            String::new()
        };

        let exe = super::process_name_for_pid(pid).unwrap_or_default();
        let title_match = !title.is_empty() && title.contains(&ctx.needle);
        let exe_match = windows_recent::app_matches_identifier(&ctx.needle, &exe);
        if !title_match && !exe_match {
            return BOOL(1);
        }

        let mut rect = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
            return BOOL(1);
        }
        let area = (rect.right - rect.left) as i64 * (rect.bottom - rect.top) as i64;
        if area <= 0 {
            return BOOL(1);
        }
        let score = area + if !title.is_empty() { 1_000_000 } else { 0 };
        let replace = ctx
            .best
            .as_ref()
            .map(|(_, prev)| score > *prev)
            .unwrap_or(true);
        if replace {
            ctx.best = Some((hwnd, score));
        }
        BOOL(1)
    }

    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
        let _ = EnumWindows(Some(find_cb), LPARAM(&mut ctx as *mut _ as isize));
    }
    Ok(ctx.best.map(|(hwnd, _)| hwnd))
}

fn find_top_level_window_for_pid(target_pid: u32, visible_only: bool) -> Option<HWND> {
    struct FindCtx {
        target_pid: u32,
        visible_only: bool,
        best: Option<(HWND, i64)>,
    }

    let mut ctx = FindCtx {
        target_pid,
        visible_only,
        best: None,
    };

    unsafe extern "system" fn find_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
            GW_OWNER,
        };

        let ctx = unsafe { &mut *(lparam.0 as *mut FindCtx) };
        if hwnd.is_invalid() {
            return BOOL(1);
        }
        if unsafe { GetWindow(hwnd, GW_OWNER) }
            .ok()
            .is_some_and(|owner| !owner.is_invalid())
        {
            return BOOL(1);
        }
        if ctx.visible_only && !unsafe { IsWindowVisible(hwnd).as_bool() } {
            return BOOL(1);
        }

        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid != ctx.target_pid {
            return BOOL(1);
        }

        let mut rect = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
            return BOOL(1);
        }
        let area = (rect.right - rect.left) as i64 * (rect.bottom - rect.top) as i64;
        if area <= 0 {
            return BOOL(1);
        }

        let mut buf = [0u16; 512];
        let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
        let score = area + if len > 0 { 1_000_000 } else { 0 };
        let replace = ctx
            .best
            .as_ref()
            .map(|(_, prev)| score > *prev)
            .unwrap_or(true);
        if replace {
            ctx.best = Some((hwnd, score));
        }
        BOOL(1)
    }

    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
        let _ = EnumWindows(Some(find_cb), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.best.map(|(hwnd, _)| hwnd)
}

fn try_relaunch_running_instance(app: &str, pid: u32) -> Result<bool> {
    let Some(path) = process_image_path_for_pid(pid) else {
        log::warn!("launch_app Windows activate: no image path for pid={pid} app={app}");
        return Ok(false);
    };
    if !path.is_file() {
        log::warn!(
            "launch_app Windows activate: image path missing on disk app={app} path={}",
            path.display()
        );
        return Ok(false);
    }

    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let path_str = path.to_string_lossy();
    log::info!(
        "launch_app Windows activate: relaunching tray instance app={app} path={path_str}"
    );
    let status = Command::new("cmd")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["/C", "start", "", &path_str])
        .status()?;
    Ok(status.success())
}

/// HWND to verify after activate — prefers a visible window.
pub fn hwnd_for_verify(app: &str) -> Result<Option<HWND>> {
    find_visible_window_for_app(app)
}
