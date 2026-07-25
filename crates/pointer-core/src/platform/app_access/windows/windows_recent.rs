//! Recent and tray-only Windows apps (UserAssist + process snapshot), aligned with macOS list_apps.

use super::super::listed_app::{compare_listed_apps, recent_usage_cutoff_days, ListedApp};
use anyhow::Result;
use chrono::{NaiveDate, TimeZone, Utc};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

pub const RECENT_USAGE_DAYS: i64 = 14;

const USERASSIST_GUIDS: &[&str] = &[
    r"{CEBFF5CD-ACE2-4F4F-9178-9926F41749EA}",
    r"{F4E2450-7455-4437-834E-1A2CB6D60366}",
];

/// Merge Start Menu installed catalog with running apps and UserAssist enrichment (14 days).
/// Start Menu is always included so launch targets (e.g. `.lnk` shortcuts) appear on the first call.
pub fn merge_list_catalog(
    visible_running: Vec<ListedApp>,
    cutoff: NaiveDate,
    include_all: bool,
) -> Result<Vec<ListedApp>> {
    let running_pids = snapshot_running_exe_pids()?;
    let visible_keys: std::collections::HashSet<String> = visible_running
        .iter()
        .map(|a| exe_key(&a.identifier))
        .collect();
    let mut by_exe: HashMap<String, ListedApp> = HashMap::new();

    for app in start_menu_installed_catalog()? {
        let key = exe_key(&app.identifier);
        by_exe.entry(key).or_insert(app);
    }

    for app in visible_running {
        let key = exe_key(&app.identifier);
        if let Some(entry) = by_exe.get_mut(&key) {
            entry.name = app.name.clone();
            entry.running = true;
            entry.frontmost = app.frontmost;
            entry.pid = app.pid;
            if app.window_title.is_some() {
                entry.window_title = app.window_title.clone();
            }
        } else {
            by_exe.insert(key, app);
        }
    }

    for (key, pid) in &running_pids {
        if visible_keys.contains(key) {
            continue;
        }
        if let Some(entry) = by_exe.get_mut(key) {
            entry.running = true;
            entry.pid = Some(*pid);
            continue;
        }
        let display = key.strip_suffix(".exe").unwrap_or(key.as_str()).to_string();
        let identifier = if key.ends_with(".exe") {
            key.clone()
        } else {
            format!("{key}.exe")
        };
        log::info!(
            "list_apps Windows: tray-only process name={display} pid={pid} exe={identifier}"
        );
        by_exe.insert(
            key.clone(),
            ListedApp {
                name: display,
                identifier,
                running: true,
                frontmost: false,
                last_used: None,
                uses: None,
                window_title: None,
                pid: Some(*pid),
            },
        );
    }

    for record in userassist_recent_apps(cutoff)? {
        let key = exe_key(&record.identifier);
        if let Some(entry) = by_exe.get_mut(&key) {
            if entry.last_used.is_none() {
                entry.last_used = record.last_used;
            }
            if entry.uses.is_none() {
                entry.uses = record.uses;
            }
            if let Some(pid) = running_pids.get(&key) {
                entry.running = true;
                entry.pid = Some(*pid);
            }
            continue;
        }
        let mut app = record;
        if let Some(pid) = running_pids.get(&key) {
            app.running = true;
            app.pid = Some(*pid);
        }
        by_exe.insert(key, app);
    }

    let mut entries: Vec<ListedApp> = by_exe.into_values().collect();
    entries.sort_by(compare_listed_apps);
    log::info!(
        "list_apps Windows: total={} running={} include_all={include_all}",
        entries.len(),
        entries.iter().filter(|e| e.running).count()
    );
    Ok(entries)
}

/// Start Menu shortcut with its on-disk path (for list + launch resolution).
#[derive(Debug, Clone)]
pub(crate) struct StartMenuLaunchEntry {
    pub path: PathBuf,
    pub name: String,
    pub identifier: String,
}

/// Full Start Menu installed catalog (.lnk and .exe shortcuts).
pub fn start_menu_installed_catalog() -> Result<Vec<ListedApp>> {
    let mut by_key: HashMap<String, ListedApp> = HashMap::new();
    for entry in collect_all_start_menu_launch_entries()? {
        let key = exe_key(&entry.identifier);
        let record = ListedApp {
            name: entry.name,
            identifier: entry.identifier,
            running: false,
            frontmost: false,
            last_used: None,
            uses: None,
            window_title: None,
            pid: None,
        };
        by_key.entry(key).or_insert(record);
    }
    Ok(by_key.into_values().collect())
}

/// Resolve a launch path from the recursive Start Menu catalog (`.lnk` / `.exe`).
pub fn resolve_start_menu_launch_path(app: &str) -> Option<PathBuf> {
    let needle = app.trim();
    if needle.is_empty() {
        return None;
    }
    let entries = collect_all_start_menu_launch_entries().ok()?;
    let picked = pick_best_launch_match(needle, &entries)?;
    log::info!(
        "resolve_start_menu_launch_path: app={needle} -> {} (identifier={})",
        picked.path.display(),
        picked.identifier
    );
    Some(picked.path.clone())
}

fn collect_all_start_menu_launch_entries() -> Result<Vec<StartMenuLaunchEntry>> {
    let mut out = Vec::new();
    for root in super::start_menu_roots() {
        collect_start_menu_launch_entries(&root, &mut out)?;
    }
    Ok(out)
}

fn collect_start_menu_launch_entries(
    dir: &Path,
    out: &mut Vec<StartMenuLaunchEntry>,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            log::warn!("list_apps Windows: cannot read {}: {e}", dir.display());
            return Ok(());
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_start_menu_launch_entries(&path, out)?;
            continue;
        }
        let Some(record) = start_menu_launch_entry(&path) else {
            continue;
        };
        out.push(record);
    }
    Ok(())
}

fn pick_best_launch_match<'a>(
    app: &str,
    entries: &'a [StartMenuLaunchEntry],
) -> Option<&'a StartMenuLaunchEntry> {
    entries
        .iter()
        .filter_map(|entry| launch_match_score(app, entry).map(|score| (score, entry)))
        .min_by_key(|(score, _)| *score)
        .map(|(_, entry)| entry)
}

fn launch_match_score(app: &str, entry: &StartMenuLaunchEntry) -> Option<u8> {
    if !entry.path.is_file() {
        return None;
    }
    let key = normalize_app_key(app);
    if key.is_empty() {
        return None;
    }
    let id_key = normalize_app_key(&entry.identifier);
    let name_key = normalize_app_key(&entry.name);
    if entry
        .path
        .to_string_lossy()
        .eq_ignore_ascii_case(app.trim())
    {
        return Some(0);
    }
    if id_key == key {
        return Some(1);
    }
    if name_key == key {
        return Some(2);
    }
    if app_matches_identifier(app, &entry.identifier) {
        return Some(3);
    }
    if app_matches_identifier(app, &entry.name) {
        return Some(4);
    }
    if entry.path.extension().and_then(|e| e.to_str()) == Some("exe") {
        if let Some(file_name) = entry.path.file_name().and_then(|s| s.to_str()) {
            if app_matches_identifier(app, file_name) {
                return Some(5);
            }
        }
    }
    None
}

fn start_menu_launch_entry(path: &Path) -> Option<StartMenuLaunchEntry> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    if ext != "lnk" && ext != "exe" {
        return None;
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())?;
    let identifier = if ext == "exe" {
        path.file_name()?.to_str()?.to_string()
    } else {
        format!("{stem}.lnk")
    };
    Some(StartMenuLaunchEntry {
        path: path.to_path_buf(),
        name: stem.to_string(),
        identifier,
    })
}

fn exe_key(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn userassist_recent_apps(cutoff: NaiveDate) -> Result<Vec<ListedApp>> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let base =
        hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\UserAssist")?;
    let mut out: HashMap<String, ListedApp> = HashMap::new();

    for guid in USERASSIST_GUIDS {
        let Ok(count_key) = base.open_subkey(format!("{guid}\\Count")) else {
            continue;
        };
        for (encoded, value) in count_key.enum_values().filter_map(|v| v.ok()) {
            let Some((uses, last_used)) = parse_userassist_value(&value.bytes) else {
                continue;
            };
            if last_used < cutoff {
                continue;
            }
            let decoded = rot13(&encoded);
            let Some(exe_name) = exe_name_from_userassist_path(&decoded) else {
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
            let key = exe_key(&exe_name);
            out.entry(key).or_insert_with(|| ListedApp {
                name: display,
                identifier: exe_name.clone(),
                running: false,
                frontmost: false,
                last_used: Some(last_used),
                uses: Some(uses),
                window_title: None,
                pid: None,
            });
        }
    }

    let mut entries: Vec<_> = out.into_values().collect();
    entries.sort_by(compare_listed_apps);
    Ok(entries)
}

fn exe_name_from_userassist_path(decoded: &str) -> Option<String> {
    let path = decoded.trim();
    if path.is_empty() {
        return None;
    }
    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path);
    if !name.to_ascii_lowercase().ends_with(".exe") {
        return None;
    }
    Some(name.to_string())
}

fn is_user_facing_exe(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if !lower.ends_with(".exe") {
        return false;
    }
    const SKIP: &[&str] = &[
        "svchost.exe",
        "conhost.exe",
        "dllhost.exe",
        "runtimebroker.exe",
        "searchhost.exe",
        "shellexperiencehost.exe",
        "startmenuexperiencehost.exe",
        "textinputhost.exe",
        "applicationframehost.exe",
        "systemsettings.exe",
        "widgetservice.exe",
        "cmd.exe",
        "powershell.exe",
        "pwsh.exe",
    ];
    !SKIP.iter().any(|s| *s == lower)
}

fn parse_userassist_value(data: &[u8]) -> Option<(i32, NaiveDate)> {
    let (run_count, filetime) = if data.len() >= 68 {
        (
            u32::from_le_bytes(data.get(4..8)?.try_into().ok()?),
            u64::from_le_bytes(data.get(60..68)?.try_into().ok()?),
        )
    } else if data.len() >= 16 {
        (
            u32::from_le_bytes(data.get(0..4)?.try_into().ok()?),
            u64::from_le_bytes(data.get(8..16)?.try_into().ok()?),
        )
    } else {
        return None;
    };
    if run_count == 0 {
        return None;
    }
    let last_used = filetime_to_naive_date(filetime)?;
    Some((run_count as i32, last_used))
}

fn filetime_to_naive_date(ft: u64) -> Option<NaiveDate> {
    if ft < 10_000_000_000_000 {
        return None;
    }
    let secs = (ft / 10_000_000).saturating_sub(11_644_473_600);
    Utc.timestamp_opt(secs as i64, 0)
        .single()
        .map(|dt| dt.date_naive())
}

fn rot13(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            'A'..='M' | 'a'..='m' => ((c as u8) + 13) as char,
            'N'..='Z' | 'n'..='z' => ((c as u8) - 13) as char,
            other => other,
        })
        .collect()
}

fn snapshot_running_exe_pids() -> Result<HashMap<String, u32>> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut out = HashMap::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
        if snap.is_invalid() {
            return Ok(out);
        }
        let _guard = HandleGuard(snap);

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_err() {
            return Ok(out);
        }
        loop {
            let pid = entry.th32ProcessID;
            if pid != 0 {
                if let Some(exe) = process_name_for_pid(pid) {
                    let key = exe_key(&exe);
                    if is_user_facing_exe(&exe) {
                        out.entry(key).or_insert(pid);
                    }
                }
            }
            if Process32NextW(snap, &mut entry).is_err() {
                break;
            }
        }
    }
    Ok(out)
}

fn process_name_for_pid(pid: u32) -> Option<String> {
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
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

struct HandleGuard(HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub fn recent_cutoff() -> NaiveDate {
    recent_usage_cutoff_days(RECENT_USAGE_DAYS)
}

/// Whether `app` (user/model identifier) matches a running executable file name.
pub fn app_matches_identifier(app: &str, exe_name: &str) -> bool {
    let needle = normalize_app_key(app);
    let hay = normalize_app_key(exe_name);
    if needle.is_empty() || hay.is_empty() {
        return false;
    }
    let needle_stem = needle.strip_suffix(".exe").unwrap_or(&needle);
    let hay_stem = hay.strip_suffix(".exe").unwrap_or(&hay);
    hay.contains(&needle)
        || needle.contains(&hay)
        || hay_stem.contains(needle_stem)
        || needle_stem.contains(hay_stem)
}

/// PID of a running user-facing process matching `app`, if any.
pub fn find_running_pid_for_app(app: &str) -> Result<Option<u32>> {
    let needle = app.trim();
    if needle.is_empty() {
        return Ok(None);
    }
    for (exe, pid) in snapshot_running_exe_pids()? {
        if app_matches_identifier(needle, &exe) {
            return Ok(Some(pid));
        }
    }
    Ok(None)
}

/// Full filesystem path for a process image.
pub fn process_image_path_for_pid(pid: u32) -> Option<PathBuf> {
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
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
        Some(PathBuf::from(String::from_utf16_lossy(
            &buf[..size as usize],
        )))
    }
}

fn normalize_app_key(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rot13_roundtrip() {
        let s = "WeChat.exe";
        assert_eq!(rot13(&rot13(s)), s);
    }

    #[test]
    fn skips_system_exe_names() {
        assert!(!is_user_facing_exe("svchost.exe"));
        assert!(is_user_facing_exe("WeChat.exe"));
    }

    #[test]
    fn app_identifier_matching() {
        assert!(app_matches_identifier("WeChat", "wechat.exe"));
        assert!(app_matches_identifier("WeChat.exe", "WeChat.exe"));
        assert!(app_matches_identifier("wechat", "WeChat.exe"));
        assert!(!app_matches_identifier("Chrome", "WeChat.exe"));
    }

    #[test]
    fn launch_match_chrome_exe_to_google_chrome_lnk() {
        let entries = vec![StartMenuLaunchEntry {
            path: PathBuf::from(
                r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Google Chrome.lnk",
            ),
            name: "Google Chrome".into(),
            identifier: "Google Chrome.lnk".into(),
        }];
        let picked =
            pick_best_launch_match("chrome.exe", &entries).expect("chrome.exe should match");
        assert_eq!(picked.identifier, "Google Chrome.lnk");
    }

    #[test]
    fn launch_match_prefers_exact_identifier() {
        let entries = vec![
            StartMenuLaunchEntry {
                path: PathBuf::from(r"C:\Start Menu\Programs\Google Chrome.lnk"),
                name: "Google Chrome".into(),
                identifier: "Google Chrome.lnk".into(),
            },
            StartMenuLaunchEntry {
                path: PathBuf::from(r"C:\Start Menu\Programs\Chrome Dev.lnk"),
                name: "Chrome Dev".into(),
                identifier: "Chrome Dev.lnk".into(),
            },
        ];
        let picked = pick_best_launch_match("Google Chrome.lnk", &entries).expect("exact lnk id");
        assert_eq!(picked.name, "Google Chrome");
    }

    #[test]
    fn launch_match_display_name() {
        let entries = vec![StartMenuLaunchEntry {
            path: PathBuf::from(r"C:\Start Menu\Programs\Google Chrome.lnk"),
            name: "Google Chrome".into(),
            identifier: "Google Chrome.lnk".into(),
        }];
        let picked = pick_best_launch_match("Google Chrome", &entries).expect("display name");
        assert_eq!(picked.identifier, "Google Chrome.lnk");
    }
}
