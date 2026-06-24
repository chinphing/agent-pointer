//! Linux application discovery and launch (Codex Computer Use aligned).

use super::launch_verify::{self, LaunchVerifyKind, LaunchVerifyOutcome, LAUNCH_VERIFY_POLL_MS};
use super::linux_recent::{
    find_desktop_id_for_app, find_running_pid_for_app, merge_list_catalog, process_exe_for_pid,
    recent_cutoff,
};
use super::types::{AppOpenOptions, AppOpenResult};
use anyhow::{anyhow, Result};
use std::process::Command;
use std::thread;
use std::time::Duration;

const TRAY_RELAUNCH_SETTLE_MS: u64 = 800;

pub fn list_apps(options: super::types::ListAppsOptions) -> Result<Vec<super::listed_app::ListedApp>> {
    let visible = wm_running_apps()?;
    merge_list_catalog(visible, recent_cutoff(), options.include_all)
}

pub fn launch_app(app: &str, options: AppOpenOptions) -> Result<AppOpenResult> {
    let app_trim = app.trim();
    if app_trim.is_empty() {
        return Err(anyhow!("app must not be empty"));
    }

    if !options.new_instance {
        if try_activate_running(app_trim)? {
            log::info!("launch_app Linux: activated app={app_trim}");
            let mut result = AppOpenResult {
                success: true,
                action: "activate".into(),
                message: format!("Activated window matching \"{app_trim}\"."),
                app_name: Some(app_trim.to_string()),
            };
            let verification = wait_for_launch_verification(app_trim, LaunchVerifyKind::Activate);
            launch_verify::apply_launch_verification(&mut result, verification);
            return Ok(result);
        }
        if options.activate_only {
            let message = if find_running_pid_for_app(app_trim)?.is_some() {
                format!(
                    "\"{app_trim}\" is running but its window could not be focused (tray-only or Wayland limitation)."
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

    launch_linux(app_trim, options.new_instance)
}

fn try_activate_running(app: &str) -> Result<bool> {
    if try_activate_via_wm(app)? {
        return Ok(true);
    }
    let Some(pid) = find_running_pid_for_app(app)? else {
        return Ok(false);
    };
    log::info!("launch_app Linux activate: tray-only pid={pid} app={app}");
    if !try_relaunch_running_instance(app, pid)? {
        log::warn!(
            "launch_app Linux activate: relaunch failed for tray-only app={app} pid={pid}"
        );
        return Ok(false);
    }
    thread::sleep(Duration::from_millis(TRAY_RELAUNCH_SETTLE_MS));
    try_activate_via_wm(app)
}

fn wm_running_apps() -> Result<Vec<super::listed_app::ListedApp>> {
    let out = Command::new("wmctrl")
        .args(["-l", "-p"])
        .output()
        .map_err(|_| anyhow!("wmctrl not available; install wmctrl for list_apps on Linux (X11)"))?;
    if !out.status.success() {
        return Err(anyhow!("wmctrl failed"));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut out_entries = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let pid = parts[2].parse::<u32>().ok();
        let title = parts[3..].join(" ");
        if title.is_empty() {
            continue;
        }
        let name = title.rsplit(" - ").next().unwrap_or(&title).trim().to_string();
        let key = format!("{}:{:?}", name.to_ascii_lowercase(), pid);
        if !seen.insert(key) {
            continue;
        }
        out_entries.push(super::listed_app::ListedApp {
            name: name.clone(),
            identifier: name,
            running: true,
            frontmost: false,
            last_used: None,
            uses: None,
            window_title: Some(if title.is_empty() {
                "untitled".into()
            } else {
                title
            }),
            pid,
        });
    }
    out_entries.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    Ok(out_entries)
}

fn try_activate_via_wm(app: &str) -> Result<bool> {
    let needle = app.to_ascii_lowercase();
    let out = Command::new("wmctrl").args(["-l"]).output();
    let Ok(out) = out else {
        return Ok(false);
    };
    if !out.status.success() {
        return Ok(false);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains(&needle) {
            let win_id = line.split_whitespace().next().unwrap_or("");
            if win_id.is_empty() {
                continue;
            }
            let status = Command::new("wmctrl").args(["-ia", win_id]).status();
            return Ok(status.map(|s| s.success()).unwrap_or(false));
        }
    }
    Ok(false)
}

fn try_relaunch_running_instance(app: &str, pid: u32) -> Result<bool> {
    if let Some(desktop_id) = find_desktop_id_for_app(app) {
        log::info!(
            "launch_app Linux activate: gtk-launch desktop_id={desktop_id} app={app}"
        );
        if try_gtk_launch(&desktop_id)? {
            return Ok(true);
        }
    }
    let Some(path) = process_exe_for_pid(pid) else {
        log::warn!("launch_app Linux activate: no exe path for pid={pid} app={app}");
        return Ok(false);
    };
    if !path.is_file() {
        log::warn!(
            "launch_app Linux activate: exe missing on disk app={app} path={}",
            path.display()
        );
        return Ok(false);
    }
    log::info!(
        "launch_app Linux activate: relaunching exe app={app} path={}",
        path.display()
    );
    let status = Command::new(&path).status()?;
    Ok(status.success())
}

fn launch_linux(app: &str, new_instance: bool) -> Result<AppOpenResult> {
    let _ = new_instance;
    if let Some(desktop_id) = find_desktop_id_for_app(app) {
        if try_gtk_launch(&desktop_id)? {
            let mut result = AppOpenResult {
                success: true,
                action: "launch".into(),
                message: format!("Launched \"{app}\" via desktop id {desktop_id}."),
                app_name: Some(app.to_string()),
            };
            let verification = wait_for_launch_verification(app, LaunchVerifyKind::Launch);
            launch_verify::apply_launch_verification(&mut result, verification);
            return Ok(result);
        }
    }
    if try_gtk_launch(app)? {
        let mut result = AppOpenResult {
            success: true,
            action: "launch".into(),
            message: format!("Launched \"{app}\" via gtk-launch."),
            app_name: Some(app.to_string()),
        };
        let verification = wait_for_launch_verification(app, LaunchVerifyKind::Launch);
        launch_verify::apply_launch_verification(&mut result, verification);
        return Ok(result);
    }

    Err(anyhow!(
        "Could not launch \"{app}\". Call list_apps and pass a running app name or .desktop id."
    ))
}

fn try_gtk_launch(desktop_id: &str) -> Result<bool> {
    let status = Command::new("gtk-launch").arg(desktop_id).status();
    Ok(status.map(|s| s.success()).unwrap_or(false))
}

pub(crate) fn wait_for_launch_verification(
    app: &str,
    kind: LaunchVerifyKind,
) -> LaunchVerifyOutcome {
    use std::time::Instant;

    let timeout_ms = kind.timeout_ms();
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if let Some(outcome) = app_process_state(app) {
            if outcome.running && outcome.frontmost {
                return outcome;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(LAUNCH_VERIFY_POLL_MS));
    }
    app_process_state(app).unwrap_or_else(LaunchVerifyOutcome::not_found)
}

fn app_process_state(app: &str) -> Option<LaunchVerifyOutcome> {
    if let Some(outcome) = wm_window_state(app) {
        return Some(outcome);
    }
    if find_running_pid_for_app(app).ok()?.is_some() {
        return Some(LaunchVerifyOutcome {
            running: true,
            frontmost: false,
        });
    }
    None
}

fn wm_window_state(app: &str) -> Option<LaunchVerifyOutcome> {
    let out = Command::new("wmctrl").args(["-l"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let needle = app.to_ascii_lowercase();
    let text = String::from_utf8_lossy(&out.stdout);
    let mut running = false;
    for line in text.lines() {
        if line.to_ascii_lowercase().contains(&needle) {
            running = true;
            break;
        }
    }
    if !running {
        return None;
    }
    let active = Command::new("wmctrl")
        .args(["-l"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .and_then(|listing| {
            listing
                .lines()
                .find(|l| l.starts_with('*'))
                .map(|l| l.to_ascii_lowercase().contains(&needle))
        })
        .unwrap_or(false);
    Some(LaunchVerifyOutcome {
        running: true,
        frontmost: active,
    })
}

/// Minimum width/height for wmctrl window geometry to count as user-visible.
const MIN_WM_WINDOW_DIMENSION: i32 = 50;

fn wmctrl_geometry_listing() -> Option<String> {
    let output = Command::new("wmctrl").args(["-lG"]).output().ok()?;
    if !output.status.success() {
        log::warn!("auto monitor switch Linux: wmctrl -lG failed");
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn window_center_from_wmctrl_lg_parts(parts: &[&str]) -> Option<(i32, i32)> {
    if parts.len() < 7 {
        return None;
    }
    let x: i32 = parts[2].parse().ok()?;
    let y: i32 = parts[3].parse().ok()?;
    let w: i32 = parts[4].parse().ok()?;
    let h: i32 = parts[5].parse().ok()?;
    if w < MIN_WM_WINDOW_DIMENSION || h < MIN_WM_WINDOW_DIMENSION {
        return None;
    }
    Some((x + w / 2, y + h / 2))
}

fn wmctrl_active_window_id() -> Option<String> {
    let output = Command::new("wmctrl").args(["-l"]).output().ok()?;
    if !output.status.success() {
        log::warn!("auto monitor switch Linux: wmctrl -l failed (Wayland or wmctrl missing?)");
        return None;
    }
    let listing = String::from_utf8_lossy(&output.stdout);
    for line in listing.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[1] == "*" {
            return Some(parts[0].to_string());
        }
    }
    None
}

fn window_center_for_wmctrl_id(win_id: &str) -> Option<(i32, i32)> {
    let listing = wmctrl_geometry_listing()?;
    for line in listing.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.first()? != &win_id {
            continue;
        }
        return window_center_from_wmctrl_lg_parts(&parts);
    }
    None
}

/// Capture monitor id for the active (focused) window when wmctrl is available (X11).
pub fn monitor_id_for_frontmost_app() -> Option<String> {
    let win_id = wmctrl_active_window_id()?;
    let (x, y) = window_center_for_wmctrl_id(&win_id)?;
    let monitor_id = crate::agents::computer::screen::monitor_id_at_global_point(x, y).ok()?;
    log::info!(
        "auto monitor switch Linux: active window {win_id} center=({x},{y}) -> monitor={monitor_id}"
    );
    Some(monitor_id)
}

/// Global screen center of the app's window when wmctrl is available.
pub fn window_center_for_app(app: &str) -> Option<(i32, i32)> {
    let needle = app.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return None;
    }
    let listing = wmctrl_geometry_listing()?;
    for line in listing.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let title = parts.get(7..)?.join(" ").to_ascii_lowercase();
        if !title.contains(&needle) {
            continue;
        }
        if let Some(center) = window_center_from_wmctrl_lg_parts(&parts) {
            return Some(center);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wmctrl_lg_parts_center() {
        let parts = ["0x1", "0", "100", "200", "800", "600", "host", "Title"];
        assert_eq!(window_center_from_wmctrl_lg_parts(&parts), Some((500, 500)));
    }

    #[test]
    fn wmctrl_lg_parts_skips_tiny_window() {
        let parts = ["0x1", "0", "0", "0", "10", "10", "host", "Title"];
        assert_eq!(window_center_from_wmctrl_lg_parts(&parts), None);
    }
}
