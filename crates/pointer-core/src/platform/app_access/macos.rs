//! macOS application discovery and launch (Codex Computer Use aligned).

#[path = "macos_window.rs"]
mod macos_window;

use super::launch_verify::{self, LaunchVerifyKind, LaunchVerifyOutcome, LAUNCH_VERIFY_POLL_MS};
use super::listed_app::{
    compare_listed_apps, parse_mdls_date, recent_usage_cutoff_days, ListedApp,
};
use super::types::{AppOpenOptions, AppOpenResult, ListAppsOptions};
use crate::platform::macos_permissions;
use anyhow::{anyhow, Result};
use chrono::NaiveDate;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const RECENT_USAGE_DAYS: i64 = 14;
const TRAY_RELAUNCH_SETTLE_MS: u64 = 800;
const ACTIVATE_SETTLE_MS: u64 = 200;

pub fn list_apps(options: ListAppsOptions) -> Result<Vec<ListedApp>> {
    if !macos_permissions::accessibility_effective() {
        return Err(anyhow!(
            "Accessibility permission required for list_apps on macOS. Enable Pointer in System Settings → Privacy & Security → Accessibility."
        ));
    }
    let include_all = options.include_all;
    crate::platform::run_synthetic_input(move || list_apps_impl(include_all))
}

fn list_apps_impl(include_all: bool) -> Result<Vec<ListedApp>> {
    use objc2_app_kit::NSWorkspace;

    let workspace = NSWorkspace::sharedWorkspace();
    let frontmost = workspace
        .frontmostApplication()
        .and_then(|app| app.bundleIdentifier())
        .map(|s| s.to_string().to_ascii_lowercase());

    let mut by_bundle: HashMap<String, ListedApp> = HashMap::new();
    if include_all {
        for app in installed_application_catalog()? {
            let key = app.identifier.to_ascii_lowercase();
            by_bundle.insert(key, app);
        }
    }

    for app in user_facing_running(&frontmost)? {
        let key = app.identifier.to_ascii_lowercase();
        if let Some(entry) = by_bundle.get_mut(&key) {
            entry.name = app.name.clone();
            entry.running = true;
            entry.frontmost = app.frontmost;
            entry.pid = app.pid;
        } else {
            by_bundle.insert(key, app);
        }
    }

    let cutoff = recent_usage_cutoff_days(RECENT_USAGE_DAYS);
    for record in spotlight_recent_apps(cutoff)? {
        let key = record.identifier.to_ascii_lowercase();
        if let Some(entry) = by_bundle.get_mut(&key) {
            if entry.last_used.is_none() {
                entry.last_used = record.last_used;
            }
            if entry.uses.is_none() {
                entry.uses = record.uses;
            }
            continue;
        }
        by_bundle.insert(key, record);
    }

    let mut entries: Vec<ListedApp> = by_bundle.into_values().collect();
    entries.sort_by(compare_listed_apps);
    log::info!(
        "list_apps macOS: total={} running={} include_all={include_all}",
        entries.len(),
        entries.iter().filter(|e| e.running).count()
    );
    Ok(entries)
}

pub fn launch_app(app: &str, options: AppOpenOptions) -> Result<AppOpenResult> {
    if !macos_permissions::accessibility_effective() {
        return Err(anyhow!(
            "Accessibility permission required for launch_app on macOS. Enable Pointer in System Settings → Privacy & Security → Accessibility."
        ));
    }
    crate::platform::run_synthetic_input(|| launch_app_impl(app, options))
}

fn launch_app_impl(app: &str, options: AppOpenOptions) -> Result<AppOpenResult> {
    let app_trim = app.trim();
    if app_trim.is_empty() {
        return Err(anyhow!("app must not be empty"));
    }

    if !options.new_instance {
        if activate_running(app_trim)? {
            log::info!("launch_app macOS: activated app={app_trim}");
            let display = display_name_for_message(app_trim);
            let mut result = AppOpenResult {
                success: true,
                action: "activate".into(),
                message: format!("Activated \"{display}\"."),
                app_name: Some(app_trim.to_string()),
            };
            let verification = wait_for_launch_verification(app_trim, LaunchVerifyKind::Activate);
            launch_verify::apply_launch_verification(&mut result, verification);
            return Ok(result);
        }
        if options.activate_only {
            return Ok(AppOpenResult {
                success: false,
                action: "activate".into(),
                message: format!("No running instance of \"{app_trim}\"."),
                app_name: Some(app_trim.to_string()),
            });
        }
    }

    launch_macos(app_trim, options.new_instance)
}

fn launch_macos(app: &str, new_instance: bool) -> Result<AppOpenResult> {
    if is_bundle_identifier(app) {
        if let Some(url) = application_url_for_bundle(app) {
            return open_application_url(&url, app, new_instance);
        }
    }

    if let Some(url) = application_url_named(app) {
        return open_application_url(&url, app, new_instance);
    }

    let mut cmd = Command::new("open");
    if new_instance {
        cmd.arg("-n");
    }
    let status = cmd.args(["-a", app]).status();
    if status.map(|s| s.success()).unwrap_or(false) {
        let verb = if new_instance {
            "Launched new instance of"
        } else {
            "Launched"
        };
        log::info!("launch_app macOS: open -a app={app} new_instance={new_instance}");
        let mut result = AppOpenResult {
            success: true,
            action: "launch".into(),
            message: format!("{verb} \"{app}\"."),
            app_name: Some(app.to_string()),
        };
        let verification = wait_for_launch_verification(app, LaunchVerifyKind::Launch);
        launch_verify::apply_launch_verification(&mut result, verification);
        return Ok(result);
    }

    Err(anyhow!(
        "Could not launch \"{app}\". Call list_apps and pass an exact app name or bundle identifier."
    ))
}

fn user_facing_running(frontmost: &Option<String>) -> Result<Vec<ListedApp>> {
    use objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};

    let workspace = NSWorkspace::sharedWorkspace();
    let apps = workspace.runningApplications();
    let mut out = Vec::new();
    let count = apps.count();
    for i in 0..count {
        let app = apps.objectAtIndex(i);
        if app.isTerminated() {
            continue;
        }
        if app.activationPolicy() != NSApplicationActivationPolicy::Regular {
            continue;
        }
        let name = app
            .localizedName()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Unknown".into());
        let bundle = app
            .bundleIdentifier()
            .map(|s| s.to_string())
            .unwrap_or_else(|| name.clone());
        let key = bundle.to_ascii_lowercase();
        let pid = app.processIdentifier();
        let ns_front = frontmost.as_ref().map(|f| f == &key).unwrap_or(false);
        let is_front = ns_front && macos_window::has_onscreen_window_for_pid(pid);
        out.push(ListedApp {
            name,
            identifier: bundle,
            running: true,
            frontmost: is_front,
            last_used: None,
            uses: None,
            window_title: None,
            pid: Some(app.processIdentifier() as u32),
        });
    }
    Ok(out)
}

fn spotlight_recent_apps(cutoff: NaiveDate) -> Result<Vec<ListedApp>> {
    let mut records = Vec::new();
    let roots = [
        "/Applications",
        "/System/Applications",
        "/System/Library/CoreServices",
    ];
    let home_apps = dirs::home_dir().map(|h| h.join("Applications"));
    for root in roots {
        records.extend(spotlight_apps_in(Path::new(root), cutoff)?);
    }
    if let Some(home) = home_apps {
        records.extend(spotlight_apps_in(&home, cutoff)?);
    }
    records.sort_by(compare_listed_apps);
    Ok(records)
}

fn installed_application_catalog() -> Result<Vec<ListedApp>> {
    let mut by_bundle: HashMap<String, ListedApp> = HashMap::new();
    let roots = [
        "/Applications",
        "/System/Applications",
        "/System/Library/CoreServices",
    ];
    let home_apps = dirs::home_dir().map(|h| h.join("Applications"));
    for root in roots {
        for record in catalog_apps_in(Path::new(root))? {
            let key = record.identifier.to_ascii_lowercase();
            by_bundle.entry(key).or_insert(record);
        }
    }
    if let Some(home) = home_apps {
        for record in catalog_apps_in(&home)? {
            let key = record.identifier.to_ascii_lowercase();
            by_bundle.entry(key).or_insert(record);
        }
    }
    Ok(by_bundle.into_values().collect())
}

fn catalog_apps_in(root: &Path) -> Result<Vec<ListedApp>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = HashSet::new();
    collect_mdfind_app_paths(root, &mut paths);
    walk_app_bundles(root, &mut paths);
    let mut out = Vec::new();
    for path in paths {
        let path_str = path.to_string_lossy().into_owned();
        let Some(record) = app_catalog_record(&path_str) else {
            continue;
        };
        out.push(record);
    }
    Ok(out)
}

fn collect_mdfind_app_paths(root: &Path, paths: &mut HashSet<PathBuf>) {
    let Ok(output) = Command::new("mdfind")
        .args([
            "-onlyin",
            &root.to_string_lossy(),
            "kMDItemContentType==com.apple.application-bundle",
        ])
        .output()
    else {
        log::warn!("list_apps macOS: mdfind unavailable for {}", root.display());
        return;
    };
    if !output.status.success() {
        return;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        paths.insert(PathBuf::from(line));
    }
}

fn walk_app_bundles(dir: &Path, paths: &mut HashSet<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("app"))
            && path.is_dir()
        {
            paths.insert(path);
            continue;
        }
        if path.is_dir() {
            walk_app_bundles(&path, paths);
        }
    }
}

fn spotlight_apps_in(root: &Path, cutoff: NaiveDate) -> Result<Vec<ListedApp>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = HashSet::new();
    collect_mdfind_app_paths(root, &mut paths);
    let mut out = Vec::new();
    for path in paths {
        let path_str = path.to_string_lossy();
        let Some(record) = mdls_app_record(&path_str, cutoff) else {
            continue;
        };
        out.push(record);
    }
    Ok(out)
}

fn app_catalog_record(path: &str) -> Option<ListedApp> {
    if let Some(record) = mdls_catalog_record(path) {
        return Some(record);
    }
    let record = plist_catalog_record(path)?;
    log::info!("list_apps macOS: mdls unavailable for {path}, using Info.plist fallback");
    Some(record)
}

fn mdls_catalog_record(path: &str) -> Option<ListedApp> {
    let output = Command::new("mdls")
        .args([
            "-name",
            "kMDItemCFBundleIdentifier",
            "-name",
            "kMDItemDisplayName",
            "-name",
            "kMDItemLastUsedDate",
            "-name",
            "kMDItemUseCount",
            path,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let bundle = mdls_value(&text, "kMDItemCFBundleIdentifier")?;
    if bundle.is_empty() {
        return None;
    }
    if is_background_bundle(path) {
        return None;
    }
    let display = mdls_value(&text, "kMDItemDisplayName")
        .map(|s| strip_app_suffix(&s))
        .or_else(|| {
            Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str().map(str::to_string))
        })?;
    let last_used = mdls_value(&text, "kMDItemLastUsedDate").and_then(|s| parse_mdls_date(&s));
    let uses = mdls_value(&text, "kMDItemUseCount").and_then(|s| s.parse().ok());
    Some(ListedApp {
        name: display,
        identifier: bundle,
        running: false,
        frontmost: false,
        last_used,
        uses,
        window_title: None,
        pid: None,
    })
}

fn mdls_app_record(path: &str, cutoff: NaiveDate) -> Option<ListedApp> {
    let record = mdls_catalog_record(path)?;
    match record.last_used {
        Some(day) if day >= cutoff => Some(record),
        _ => None,
    }
}

fn plist_catalog_record(path: &str) -> Option<ListedApp> {
    if is_background_bundle(path) {
        return None;
    }
    let text = read_bundle_plist_text(path)?;
    let bundle = plist_string_value(&text, "CFBundleIdentifier")?;
    if bundle.is_empty() {
        return None;
    }
    let display = plist_string_value(&text, "CFBundleDisplayName")
        .or_else(|| plist_string_value(&text, "CFBundleName"))
        .map(|s| strip_app_suffix(&s))
        .or_else(|| {
            Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str().map(str::to_string))
        })?;
    Some(ListedApp {
        name: display,
        identifier: bundle,
        running: false,
        frontmost: false,
        last_used: None,
        uses: None,
        window_title: None,
        pid: None,
    })
}

fn read_bundle_plist_text(path: &str) -> Option<String> {
    let plist = PathBuf::from(path).join("Contents/Info.plist");
    let bytes = fs::read(plist).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn plist_string_value(text: &str, key: &str) -> Option<String> {
    let marker = format!("<key>{key}</key>");
    let pos = text.find(&marker)?;
    let after = text[pos + marker.len()..].trim_start();
    let value = after.strip_prefix("<string>")?;
    let end = value.find("</string>")?;
    let s = value[..end].trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn is_background_bundle(path: &str) -> bool {
    let Some(text) = read_bundle_plist_text(path) else {
        return false;
    };
    plist_bool_true(&text, "LSBackgroundOnly") || plist_bool_true(&text, "LSUIElement")
}

fn plist_bool_true(text: &str, key: &str) -> bool {
    let marker = format!("<key>{key}</key>");
    let Some(pos) = text.find(&marker) else {
        return false;
    };
    let after = text[pos + marker.len()..].trim_start();
    after.starts_with("<true/>") || after.starts_with("<true />")
}

fn mdls_value(text: &str, key: &str) -> Option<String> {
    let needle = format!("{key} = ");
    let start = text.find(&needle)? + needle.len();
    let rest = text[start..].lines().next()?.trim();
    if rest == "(null)" {
        return None;
    }
    Some(rest.trim_matches('"').to_string())
}

fn activate_running(app: &str) -> Result<bool> {
    use objc2_app_kit::{
        NSApplicationActivationOptions, NSApplicationActivationPolicy, NSWorkspace,
    };
    use std::thread;
    use std::time::Duration;

    let workspace = NSWorkspace::sharedWorkspace();
    let apps = workspace.runningApplications();
    let count = apps.count();
    for i in 0..count {
        let running = apps.objectAtIndex(i);
        if running.isTerminated() {
            continue;
        }
        if running.activationPolicy() != NSApplicationActivationPolicy::Regular {
            continue;
        }
        let localized = running.localizedName().map(|s| s.to_string());
        let bundle = running.bundleIdentifier().map(|s| s.to_string());
        if !app_identifier_matches(app, localized.as_deref(), bundle.as_deref()) {
            continue;
        }
        let ok = running.activateWithOptions(NSApplicationActivationOptions(0));
        if ok {
            thread::sleep(Duration::from_millis(ACTIVATE_SETTLE_MS));
            if running_app_state(app).is_some_and(|o| o.frontmost) {
                return Ok(true);
            }
            log::info!(
                "launch_app macOS activate: NSWorkspace activate did not frontmost app={app}"
            );
        }
    }

    if running_app_state(app).is_some_and(|o| o.running) {
        log::info!("launch_app macOS activate: trying open -a tray restore app={app}");
        let status = Command::new("open").args(["-a", app]).status()?;
        if status.success() {
            thread::sleep(Duration::from_millis(TRAY_RELAUNCH_SETTLE_MS));
            return Ok(running_app_state(app).is_some_and(|o| o.running && o.frontmost));
        }
    }
    Ok(false)
}

fn localized_name_matches(app: &str, localized: &str) -> bool {
    let needle_key = app.trim().to_ascii_lowercase();
    let needle_stem = strip_app_suffix(app).to_ascii_lowercase();
    let name_key = localized.trim().to_ascii_lowercase();
    let name_stem = strip_app_suffix(localized).to_ascii_lowercase();
    needle_key == name_key || needle_stem == name_stem
}

fn bundle_identifier_matches(app: &str, bundle: &str) -> bool {
    let needle_key = app.trim().to_ascii_lowercase();
    let needle_stem = strip_app_suffix(app).to_ascii_lowercase();
    let bundle_key = bundle.trim().to_ascii_lowercase();
    if needle_key.is_empty() || bundle_key.is_empty() {
        return false;
    }
    if is_bundle_identifier(app) && bundle_key == needle_key {
        return true;
    }
    if bundle_key == needle_key || bundle_key == needle_stem {
        return true;
    }
    let bundle_tail = bundle_key.rsplit('.').next().unwrap_or(&bundle_key);
    if bundle_tail == needle_stem || bundle_tail == needle_key {
        return true;
    }
    if needle_stem.len() >= 4
        && bundle_tail.len() >= 4
        && (needle_stem.contains(bundle_tail) || bundle_tail.contains(&needle_stem))
    {
        return true;
    }
    false
}

fn app_identifier_matches(app: &str, localized: Option<&str>, bundle: Option<&str>) -> bool {
    let needle = app.trim();
    if needle.is_empty() {
        return false;
    }
    if localized.is_some_and(|name| localized_name_matches(needle, name)) {
        return true;
    }
    bundle.is_some_and(|b| bundle_identifier_matches(needle, b))
}

/// Localized app name for messages when a running instance matches `app`.
fn localized_display_name(app: &str) -> Option<String> {
    use objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};

    let workspace = NSWorkspace::sharedWorkspace();
    let apps = workspace.runningApplications();
    let count = apps.count();
    for i in 0..count {
        let running = apps.objectAtIndex(i);
        if running.isTerminated() {
            continue;
        }
        if running.activationPolicy() != NSApplicationActivationPolicy::Regular {
            continue;
        }
        let localized = running.localizedName().map(|s| s.to_string());
        let bundle = running
            .bundleIdentifier()
            .map(|s| s.to_string())
            .unwrap_or_default();
        if app_identifier_matches(app, localized.as_deref(), Some(bundle.as_str())) {
            return localized.filter(|s| !s.is_empty());
        }
    }
    None
}

fn display_name_for_message(app: &str) -> String {
    localized_display_name(app).unwrap_or_else(|| app.trim().to_string())
}

/// Poll until the target app is running and frontmost.
pub(crate) fn wait_for_launch_verification(
    app: &str,
    kind: LaunchVerifyKind,
) -> LaunchVerifyOutcome {
    use std::thread;
    use std::time::{Duration, Instant};

    let timeout_ms = kind.timeout_ms();
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if let Some(outcome) = running_app_state(app) {
            if outcome.running && outcome.frontmost {
                return outcome;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(LAUNCH_VERIFY_POLL_MS));
    }
    running_app_state(app).unwrap_or_else(LaunchVerifyOutcome::not_found)
}

fn running_app_state(app: &str) -> Option<LaunchVerifyOutcome> {
    use objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};

    let workspace = NSWorkspace::sharedWorkspace();
    let frontmost = workspace
        .frontmostApplication()
        .and_then(|a| a.bundleIdentifier())
        .map(|s| s.to_string().to_ascii_lowercase());
    let apps = workspace.runningApplications();
    let count = apps.count();
    for i in 0..count {
        let running = apps.objectAtIndex(i);
        if running.isTerminated() {
            continue;
        }
        if running.activationPolicy() != NSApplicationActivationPolicy::Regular {
            continue;
        }
        let localized = running.localizedName().map(|s| s.to_string());
        let bundle = running
            .bundleIdentifier()
            .map(|s| s.to_string())
            .unwrap_or_default();
        if !app_identifier_matches(app, localized.as_deref(), Some(bundle.as_str())) {
            continue;
        }
        let key = bundle.to_ascii_lowercase();
        let pid = running.processIdentifier();
        let ns_front = frontmost.as_ref().map(|f| f == &key).unwrap_or(false);
        let is_front = ns_front && macos_window::has_onscreen_window_for_pid(pid);
        if ns_front && !is_front {
            log::info!(
                "launch_app macOS verify: NSWorkspace frontmost but no on-screen window app={app} pid={pid}"
            );
        }
        return Some(LaunchVerifyOutcome {
            running: true,
            frontmost: is_front,
        });
    }
    None
}

fn is_bundle_identifier(app: &str) -> bool {
    app.contains('.')
}

fn strip_app_suffix(name: &str) -> String {
    name.strip_suffix(".app").unwrap_or(name).to_string()
}

/// Resolve the capture monitor id for a launched app (macOS: CGDisplayBounds in quartz space).
pub fn monitor_id_for_launched_app(app: &str) -> Option<String> {
    let pid = crate::platform::run_synthetic_input(|| {
        running_app_pid(app).or_else(|| {
            use objc2_app_kit::NSWorkspace;
            NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .map(|app| app.processIdentifier())
        })
    })?;
    macos_window::monitor_id_for_pid(pid).or_else(|| {
        log::warn!("launch_app macOS: monitor id unavailable for app={app} pid={pid}");
        None
    })
}

/// Capture monitor id for the frontmost app's largest visible window.
pub fn monitor_id_for_frontmost_app() -> Option<String> {
    let pid = crate::platform::run_synthetic_input(|| {
        use objc2_app_kit::NSWorkspace;
        NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .map(|app| app.processIdentifier())
    })?;
    macos_window::monitor_id_for_pid(pid)
}

/// Global screen center of the app's largest visible window (top-left origin).
pub fn window_center_for_app(app: &str) -> Option<(i32, i32)> {
    crate::platform::run_synthetic_input(|| window_center_for_app_on_main(app))
}

fn window_center_for_app_on_main(app: &str) -> Option<(i32, i32)> {
    running_app_pid(app)
        .and_then(|pid| macos_window::primary_window_center_top_left_for_pid(pid))
        .or_else(|| {
            log::info!("launch_app macOS: falling back to frontmost window center for app={app}");
            macos_window::frontmost_window_center_top_left()
        })
}

fn running_app_pid(app: &str) -> Option<i32> {
    use objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};

    let workspace = NSWorkspace::sharedWorkspace();
    let apps = workspace.runningApplications();
    let count = apps.count();
    for i in 0..count {
        let running = apps.objectAtIndex(i);
        if running.isTerminated() {
            continue;
        }
        if running.activationPolicy() != NSApplicationActivationPolicy::Regular {
            continue;
        }
        let localized = running.localizedName().map(|s| s.to_string());
        let bundle = running
            .bundleIdentifier()
            .map(|s| s.to_string())
            .unwrap_or_default();
        if app_identifier_matches(app, localized.as_deref(), Some(bundle.as_str())) {
            return Some(running.processIdentifier());
        }
    }
    None
}

fn application_url_for_bundle(bundle: &str) -> Option<PathBuf> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;

    let workspace = NSWorkspace::sharedWorkspace();
    let id = NSString::from_str(bundle);
    let url = workspace.URLForApplicationWithBundleIdentifier(&id)?;
    Some(PathBuf::from(url.path()?.to_string()))
}

fn application_url_named(name: &str) -> Option<PathBuf> {
    let target = strip_app_suffix(name);
    let mut roots = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }
    for root in roots {
        let candidate = root.join(format!("{target}.app"));
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

fn open_application_url(url: &Path, app: &str, new_instance: bool) -> Result<AppOpenResult> {
    let mut cmd = Command::new("open");
    if new_instance {
        cmd.arg("-n");
    }
    let status = cmd.arg(url).status();
    if status.map(|s| s.success()).unwrap_or(false) {
        let verb = if new_instance {
            "Launched new instance of"
        } else {
            "Launched"
        };
        let mut result = AppOpenResult {
            success: true,
            action: "launch".into(),
            message: format!("{verb} \"{app}\" at {}.", url.display()),
            app_name: Some(app.to_string()),
        };
        let verification = wait_for_launch_verification(app, LaunchVerifyKind::Launch);
        launch_verify::apply_launch_verification(&mut result, verification);
        return Ok(result);
    }
    Err(anyhow!("Failed to open {}", url.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
  <dict>
    <key>CFBundleDisplayName</key>
    <string>BaiduNetdisk</string>
    <key>CFBundleIdentifier</key>
    <string>com.baidu.netdisk</string>
    <key>CFBundleName</key>
    <string>BaiduNetdisk</string>
  </dict>
</plist>"#;

    #[test]
    fn plist_string_value_reads_bundle_fields() {
        assert_eq!(
            plist_string_value(SAMPLE_PLIST, "CFBundleIdentifier").as_deref(),
            Some("com.baidu.netdisk")
        );
        assert_eq!(
            plist_string_value(SAMPLE_PLIST, "CFBundleDisplayName").as_deref(),
            Some("BaiduNetdisk")
        );
    }

    #[test]
    fn plist_bool_true_detects_background_keys() {
        let bg = "<key>LSUIElement</key>\n\t<true/>";
        assert!(plist_bool_true(bg, "LSUIElement"));
        assert!(!plist_bool_true(bg, "LSBackgroundOnly"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn baidu_netdisk_catalog_from_filesystem_when_mdls_unavailable() {
        let path = "/Applications/BaiduNetdisk.app";
        if !Path::new(path).is_dir() {
            return;
        }
        let record = app_catalog_record(path).expect("BaiduNetdisk should be cataloged");
        assert_eq!(record.identifier, "com.baidu.netdisk");
        assert_eq!(record.name, "BaiduNetdisk");
    }

    #[test]
    fn app_identifier_rejects_pointer_app_for_baidu_netdisk() {
        assert!(!app_identifier_matches(
            "BaiduNetdisk",
            Some("pointer-app"),
            Some("com.tauri.dev")
        ));
        assert!(!app_identifier_matches(
            "BaiduNetdisk",
            Some("Pointer"),
            Some("com.pointer.app")
        ));
    }

    #[test]
    fn app_identifier_accepts_baidu_netdisk_bundle() {
        assert!(app_identifier_matches(
            "BaiduNetdisk",
            Some("百度网盘"),
            Some("com.baidu.netdisk")
        ));
        assert!(localized_name_matches("BaiduNetdisk", "BaiduNetdisk"));
        assert!(bundle_identifier_matches(
            "BaiduNetdisk",
            "com.baidu.netdisk"
        ));
    }
}
