//! Recent and tray-only Linux apps (/proc snapshot + recently-used.xbel).

use super::linux_xbel::{app_matches_identifier, normalize_key, parse_xbel_recent_apps, xbel_path};
use super::listed_app::{compare_listed_apps, recent_usage_cutoff_days, ListedApp};
use anyhow::Result;
use chrono::NaiveDate;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const RECENT_USAGE_DAYS: i64 = 14;

/// Merge desktop installed catalog with running apps and XBEL enrichment (14 days).
pub fn merge_list_catalog(
    visible_running: Vec<ListedApp>,
    cutoff: NaiveDate,
    include_all: bool,
) -> Result<Vec<ListedApp>> {
    let running_pids = snapshot_running_exe_pids()?;
    let visible_pids: HashSet<u32> = visible_running.iter().filter_map(|a| a.pid).collect();
    let mut by_key: HashMap<String, ListedApp> = HashMap::new();

    if include_all {
        for app in desktop_installed_catalog()? {
            let key = normalize_key(&app.identifier);
            by_key.insert(key, app);
        }
    }

    for app in visible_running {
        let key = normalize_key(&app.identifier);
        if let Some(entry) = by_key.get_mut(&key) {
            entry.name = app.name.clone();
            entry.running = true;
            entry.frontmost = app.frontmost;
            entry.pid = app.pid;
            if app.window_title.is_some() {
                entry.window_title = app.window_title.clone();
            }
        } else {
            by_key.insert(key, app);
        }
    }

    for (key, (display, exe_name, pid)) in &running_pids {
        if visible_pids.contains(pid) {
            continue;
        }
        if let Some(entry) = by_key.get_mut(key) {
            entry.running = true;
            entry.pid = Some(*pid);
            continue;
        }
        log::info!("list_apps Linux: tray-only process name={display} pid={pid} exe={exe_name}");
        by_key.insert(
            key.clone(),
            ListedApp {
                name: display.clone(),
                identifier: exe_name.clone(),
                running: true,
                frontmost: false,
                last_used: None,
                uses: None,
                window_title: None,
                pid: Some(*pid),
            },
        );
    }

    for record in xbel_recent_apps(cutoff)? {
        let key = normalize_key(&record.identifier);
        if let Some(entry) = by_key.get_mut(&key) {
            if entry.last_used.is_none() {
                entry.last_used = record.last_used;
            }
            if let Some(pid) = running_pids.get(&key).map(|(_, _, pid)| *pid) {
                entry.running = true;
                entry.pid = Some(pid);
            }
            continue;
        }
        let mut app = record;
        if let Some(pid) = running_pids.get(&key).map(|(_, _, pid)| *pid) {
            app.running = true;
            app.pid = Some(pid);
        }
        by_key.insert(key, app);
    }

    let mut entries: Vec<ListedApp> = by_key.into_values().collect();
    entries.sort_by(compare_listed_apps);
    log::info!(
        "list_apps Linux: total={} running={} include_all={include_all}",
        entries.len(),
        entries.iter().filter(|e| e.running).count()
    );
    Ok(entries)
}

/// Full `.desktop` installed catalog from standard application directories.
pub fn desktop_installed_catalog() -> Result<Vec<ListedApp>> {
    let mut by_key: HashMap<String, ListedApp> = HashMap::new();
    for dir in desktop_app_dirs() {
        scan_desktop_dir(&dir, &mut by_key)?;
    }
    Ok(by_key.into_values().collect())
}

fn scan_desktop_dir(dir: &Path, by_key: &mut HashMap<String, ListedApp>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            log::warn!("list_apps Linux: cannot read {}: {e}", dir.display());
            return Ok(());
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
            continue;
        }
        let Some(record) = parse_desktop_catalog_entry(&path) else {
            continue;
        };
        let key = normalize_key(&record.identifier);
        by_key.entry(key).or_insert(record);
    }
    Ok(())
}

fn parse_desktop_catalog_entry(path: &Path) -> Option<ListedApp> {
    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())?
        .to_string();
    let content = fs::read_to_string(path).ok()?;
    let mut name: Option<String> = None;
    let mut hidden = false;
    let mut no_display = false;
    let mut app_type: Option<String> = None;
    for line in content.lines() {
        if let Some(value) = line.strip_prefix("Name=") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                name = Some(trimmed.to_string());
            }
        } else if let Some(value) = line.strip_prefix("Hidden=") {
            hidden = value.trim().eq_ignore_ascii_case("true");
        } else if let Some(value) = line.strip_prefix("NoDisplay=") {
            no_display = value.trim().eq_ignore_ascii_case("true");
        } else if let Some(value) = line.strip_prefix("Type=") {
            app_type = Some(value.trim().to_string());
        }
    }
    if hidden || no_display {
        return None;
    }
    if let Some(ref kind) = app_type {
        if !kind.eq_ignore_ascii_case("Application") {
            return None;
        }
    }
    let display = name.unwrap_or_else(|| id.clone());
    Some(ListedApp {
        name: display,
        identifier: id,
        running: false,
        frontmost: false,
        last_used: None,
        uses: None,
        window_title: None,
        pid: None,
    })
}

fn xbel_recent_apps(cutoff: NaiveDate) -> Result<Vec<ListedApp>> {
    let path = xbel_path();
    let Ok(content) = fs::read_to_string(&path) else {
        log::warn!(
            "list_apps Linux: recently-used.xbel not readable at {}",
            path.display()
        );
        return Ok(Vec::new());
    };
    let apps = parse_xbel_recent_apps(&content, cutoff);
    log::info!(
        "list_apps Linux: {} recent desktop apps from {}",
        apps.len(),
        path.display()
    );
    Ok(apps)
}

fn snapshot_running_exe_pids() -> Result<HashMap<String, (String, String, u32)>> {
    let mut out = HashMap::new();
    let proc_dir = Path::new("/proc");
    let Ok(entries) = fs::read_dir(proc_dir) else {
        log::warn!("list_apps Linux: /proc not readable");
        return Ok(out);
    };
    let uid = current_uid();

    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        if pid == 0 {
            continue;
        }
        let proc_path = proc_dir.join(name);
        if !process_owned_by_uid(&proc_path, uid) {
            continue;
        }
        let Some(exe_name) = process_exe_name(&proc_path) else {
            continue;
        };
        if !is_user_facing_exe(&exe_name) {
            continue;
        }
        let display = Path::new(&exe_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&exe_name)
            .to_string();
        let key = normalize_key(&exe_name);
        out.entry(key).or_insert((display, exe_name, pid));
    }
    Ok(out)
}

fn current_uid() -> u32 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            for line in status.lines() {
                if let Some(rest) = line.strip_prefix("Uid:") {
                    return rest.split_whitespace().next()?.parse().ok();
                }
            }
            None
        })
        .unwrap_or(0)
}

fn process_owned_by_uid(proc_path: &Path, uid: u32) -> bool {
    let Ok(status) = fs::read_to_string(proc_path.join("status")) else {
        return false;
    };
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            if let Some(first) = rest.split_whitespace().next() {
                return first.parse::<u32>().ok() == Some(uid);
            }
        }
    }
    false
}

fn process_exe_name(proc_path: &Path) -> Option<String> {
    let exe = fs::read_link(proc_path.join("exe")).ok()?;
    exe.file_name().and_then(|s| s.to_str()).map(str::to_string)
}

fn is_user_facing_exe(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    const SKIP: &[&str] = &[
        "systemd",
        "kworker",
        "kthreadd",
        "migration",
        "rcu_",
        "bash",
        "sh",
        "zsh",
        "fish",
        "sshd",
        "dbus-daemon",
        "dbus-broker",
        "pipewire",
        "wireplumber",
        "xdg-desktop-portal",
        "xdg-document-portal",
        "xdg-permission-store",
        "at-spi-bus-launcher",
        "at-spi2-registryd",
        "gcr-ssh-agent",
        "ssh-agent",
        "gvfsd",
        "gvfs-",
        "dconf-service",
        "Xorg",
        "Xwayland",
        "gnome-shell",
        "kwin",
        "plasmashell",
        "sway",
        "cursor",
        "code",
        "node",
        "npm",
        "cargo",
        "rustc",
        "pointer",
    ];
    if lower.is_empty() {
        return false;
    }
    if lower.starts_with('[') {
        return false;
    }
    !SKIP.iter().any(|s| {
        if s.ends_with('-') {
            lower.starts_with(s)
        } else {
            lower == *s
        }
    })
}

pub fn recent_cutoff() -> NaiveDate {
    recent_usage_cutoff_days(RECENT_USAGE_DAYS)
}

/// PID of a running user-facing process matching `app`, if any.
pub fn find_running_pid_for_app(app: &str) -> Result<Option<u32>> {
    let needle = app.trim();
    if needle.is_empty() {
        return Ok(None);
    }
    for (exe, (_, _, pid)) in snapshot_running_exe_pids()? {
        if app_matches_identifier(needle, &exe) {
            return Ok(Some(pid));
        }
    }
    Ok(None)
}

/// Resolved executable path for a process.
pub fn process_exe_for_pid(pid: u32) -> Option<PathBuf> {
    fs::read_link(format!("/proc/{pid}/exe")).ok()
}

/// Resolve a `.desktop` id for gtk-launch from app name or desktop id.
pub fn find_desktop_id_for_app(app: &str) -> Option<String> {
    let needle = app.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return None;
    }
    for dir in desktop_app_dirs() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Some(id) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            if id.to_ascii_lowercase() == needle {
                return Some(id);
            }
            if let Some(name) = desktop_name(&path) {
                let name_lower = name.to_ascii_lowercase();
                if name_lower == needle
                    || name_lower.contains(&needle)
                    || needle.contains(&name_lower)
                {
                    return Some(id);
                }
            }
        }
    }
    None
}

fn desktop_name(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if let Some(value) = line.strip_prefix("Name=") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn desktop_app_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/share/applications"));
    }
    dirs.push(PathBuf::from("/usr/local/share/applications"));
    dirs.push(PathBuf::from("/usr/share/applications"));
    if let Ok(data_dirs) = std::env::var("XDG_DATA_DIRS") {
        for part in data_dirs.split(':').filter(|s| !s.is_empty()) {
            dirs.push(PathBuf::from(part).join("applications"));
        }
    }
    dirs
}
