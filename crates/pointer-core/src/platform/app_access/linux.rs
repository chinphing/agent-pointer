//! Linux application discovery and launch (Codex Computer Use aligned).

mod linux_recent;

use super::launch_verify::{self, LaunchVerifyKind, LaunchVerifyOutcome, LAUNCH_VERIFY_POLL_MS};
use super::types::{AppOpenOptions, AppOpenResult};
use anyhow::{anyhow, Result};
use linux_recent::{
    find_desktop_id_for_app, find_running_pid_for_app, merge_list_catalog, process_exe_for_pid,
    recent_cutoff,
};
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
        let name = title.split(" - ").next_back().unwrap_or(&title).trim().to_string();
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
