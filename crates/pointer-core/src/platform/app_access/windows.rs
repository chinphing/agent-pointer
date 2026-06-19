//! Windows application discovery and launch (Codex Computer Use aligned).

mod windows_activate;
mod windows_recent;

use super::listed_app::ListedApp;
use super::launch_verify::{self, LaunchVerifyKind, LaunchVerifyOutcome, LAUNCH_VERIFY_POLL_MS};
use super::types::{AppOpenOptions, AppOpenResult, ListAppsOptions};
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use windows_recent::find_running_pid_for_app;

pub fn list_apps(options: ListAppsOptions) -> Result<Vec<ListedApp>> {
    let visible = running_process_apps()?;
    windows_recent::merge_list_catalog(
        visible,
        windows_recent::recent_cutoff(),
        options.include_all,
    )
}

pub fn launch_app(app: &str, options: AppOpenOptions) -> Result<AppOpenResult> {
    let app_trim = app.trim();
    if app_trim.is_empty() {
        return Err(anyhow!("app must not be empty"));
    }

    if !options.new_instance {
        if let Some(hwnd) = windows_activate::try_activate_running(app_trim)? {
            focus_window(hwnd)?;
            log::info!("launch_app Windows: focused app={app_trim}");
            let mut result = AppOpenResult {
                success: true,
                action: "activate".into(),
                message: format!("Brought \"{app_trim}\" to the foreground."),
                app_name: Some(app_trim.to_string()),
            };
            let verification = wait_for_launch_verification(app_trim, LaunchVerifyKind::Activate);
            launch_verify::apply_launch_verification(&mut result, verification);
            return Ok(result);
        }
        if options.activate_only {
            let message = if find_running_pid_for_app(app_trim)?.is_some() {
                format!(
                    "\"{app_trim}\" is running but its window could not be restored (tray-only or hidden)."
                )
            } else {
                format!("No running instance found for \"{app_trim}\".")
            };
            return Ok(AppOpenResult {
                success: false,
                action: "activate".into(),
                message,
                app_name: Some(app_trim.to_string()),
            });
        }
    }

    let target = resolve_launch_target(app_trim).or_else(|| {
        find_running_pid_for_app(app_trim)
            .ok()
            .flatten()
            .and_then(windows_recent::process_image_path_for_pid)
    });
    let Some(path) = target else {
        return Err(anyhow!(
            "Could not resolve launch target for \"{app_trim}\". Call list_apps and pass an executable name such as notepad.exe."
        ));
    };
    launch_path(&path, app_trim, options.new_instance)
}

fn running_process_apps() -> Result<Vec<ListedApp>> {
    use std::collections::HashMap;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };

    struct Ctx {
        by_pid: HashMap<u32, ListedApp>,
    }

    let mut ctx = Ctx {
        by_pid: HashMap::new(),
    };

    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};

        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        if hwnd.0 == 0 || !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let mut buf = [0u16; 512];
        let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
        if len == 0 {
            return BOOL(1);
        }
        let title = String::from_utf16_lossy(&buf[..len as usize]).trim().to_string();
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == 0 {
            return BOOL(1);
        }
        let process_name = process_name_for_pid(pid).unwrap_or_else(|| format!("pid-{pid}"));
        let entry = ctx.by_pid.entry(pid).or_insert_with(|| ListedApp {
            name: process_name.clone(),
            identifier: process_name.clone(),
            running: true,
            frontmost: false,
            last_used: None,
            uses: None,
            window_title: Some(if title.is_empty() {
                "untitled".into()
            } else {
                title.clone()
            }),
            pid: Some(pid),
        });
        if title.len() > entry.window_title.as_ref().map(|s| s.len()).unwrap_or(0) {
            entry.window_title = Some(if title.is_empty() {
                "untitled".into()
            } else {
                title
            });
        }
        BOOL(1)
    }

    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut ctx as *mut _ as isize));
    }

    let mut out: Vec<_> = ctx.by_pid.into_values().collect();
    out.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    Ok(out)
}

pub(crate) fn process_name_for_pid(pid: u32) -> Option<String> {
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return None;
        };
        let _guard = HandleGuard(handle);
        let mut buf = [0u16; 512];
        let mut size = buf.len() as u32;
        if QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
        .is_err()
        {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        Path::new(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::to_string)
    }
}

fn focus_window(hwnd: windows::Win32::Foundation::HWND) -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP, VK_MENU};
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };
    unsafe {
        ShowWindow(hwnd, SW_RESTORE);
        BringWindowToTop(hwnd);
        keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        SetForegroundWindow(hwnd)?;
    }
    Ok(())
}

fn resolve_launch_target(app: &str) -> Option<PathBuf> {
    if app.contains('\\') || app.contains('/') || app.ends_with(".exe") {
        let path = PathBuf::from(app);
        if path.exists() {
            return Some(path);
        }
    }
    let lower = app.to_ascii_lowercase();
    let with_ext = if lower.ends_with(".exe") {
        lower.clone()
    } else {
        format!("{lower}.exe")
    };
    for root in start_menu_roots() {
        let candidate = root.join(&with_ext);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    if let Some(system32) = std::env::var_os("SystemRoot") {
        let candidate = PathBuf::from(system32).join("System32").join(&with_ext);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub(crate) fn start_menu_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(program_data) = std::env::var_os("ProgramData") {
        roots.push(
            PathBuf::from(program_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(
            PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    roots
}

struct HandleGuard(windows::Win32::Foundation::HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

fn launch_path(path: &Path, app: &str, new_instance: bool) -> Result<AppOpenResult> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let path_str = path.to_string_lossy();
    let status = if path.extension().and_then(|e| e.to_str()) == Some("lnk") {
        Command::new("cmd")
            .creation_flags(CREATE_NO_WINDOW)
            .args(["/C", "start", "", &path_str])
            .status()
    } else {
        Command::new(&path).status()
    }
    .map_err(|e| anyhow!("failed to launch {path_str}: {e}"))?;

    if status.success() {
        log::info!("launch_app Windows: launched path={path_str} new_instance={new_instance}");
        let verb = if new_instance {
            "Started new instance of"
        } else {
            "Started"
        };
        let mut result = AppOpenResult {
            success: true,
            action: "launch".into(),
            message: format!("{verb} \"{app}\" via {path_str}."),
            app_name: Some(app.to_string()),
        };
        let verification = wait_for_launch_verification(app, LaunchVerifyKind::Launch);
        launch_verify::apply_launch_verification(&mut result, verification);
        Ok(result)
    } else {
        Err(anyhow!("Launch failed for {path_str} (exit={status})"))
    }
}

pub(crate) fn wait_for_launch_verification(
    app: &str,
    kind: LaunchVerifyKind,
) -> LaunchVerifyOutcome {
    use std::thread;
    use std::time::{Duration, Instant};

    let timeout_ms = kind.timeout_ms();
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if let Ok(Some(outcome)) = window_state_for_app(app) {
            if outcome.running && outcome.frontmost {
                return outcome;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(LAUNCH_VERIFY_POLL_MS));
    }
    window_state_for_app(app)
        .ok()
        .flatten()
        .unwrap_or_else(LaunchVerifyOutcome::not_found)
}

/// Global screen center of a visible window (top-left origin).
pub fn window_center_for_hwnd(hwnd: windows::Win32::Foundation::HWND) -> Option<(i32, i32)> {
    use std::mem::MaybeUninit;
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    if hwnd.0 == 0 {
        return None;
    }
    let mut rect = MaybeUninit::<RECT>::uninit();
    unsafe {
        GetWindowRect(hwnd, rect.as_mut_ptr()).ok()?;
        let rect = rect.assume_init();
        let w = rect.right - rect.left;
        let h = rect.bottom - rect.top;
        if w <= 0 || h <= 0 {
            return None;
        }
        Some(((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2))
    }
}

/// Capture monitor id for the foreground app's window center.
pub fn monitor_id_for_frontmost_app() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindowVisible};

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0 == 0 || !unsafe { IsWindowVisible(hwnd).as_bool() } {
        log::warn!("auto monitor switch Windows: no visible foreground window");
        return None;
    }
    let (x, y) = window_center_for_hwnd(hwnd)?;
    let monitor_id = crate::agents::computer::screen::monitor_id_at_global_point(x, y).ok()?;
    log::info!(
        "auto monitor switch Windows: foreground window center=({x},{y}) -> monitor={monitor_id}"
    );
    Some(monitor_id)
}

/// Global screen center of the app's visible window (top-left origin).
pub fn window_center_for_app(app: &str) -> Option<(i32, i32)> {
    let hwnd = windows_activate::hwnd_for_verify(app).ok()??;
    window_center_for_hwnd(hwnd)
}

fn window_state_for_app(app: &str) -> Result<Option<LaunchVerifyOutcome>> {
    if let Some(hwnd) = windows_activate::hwnd_for_verify(app)? {
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let foreground = unsafe { GetForegroundWindow() };
        return Ok(Some(LaunchVerifyOutcome {
            running: true,
            frontmost: foreground == hwnd,
        }));
    }
    if find_running_pid_for_app(app)?.is_some() {
        return Ok(Some(LaunchVerifyOutcome {
            running: true,
            frontmost: false,
        }));
    }
    Ok(None)
}
