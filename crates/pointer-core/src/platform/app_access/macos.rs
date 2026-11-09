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
    // **不要**把整段包进主线程：`run_synthetic_input` 是同步 hop，主线程是
    // WebView 的绘制线程，占住它界面就卡死。这里真正必须上主线程的只有
    // NSWorkspace 的几次读，其余（mdfind 子进程、逐个 mdls、目录扫描）都是
    // 纯 I/O，留在调用线程。
    list_apps_impl(include_all)
}

/// 前台应用的 bundle id —— 只有这一次 NSWorkspace 读需要主线程。
fn frontmost_bundle_id() -> Option<String> {
    use objc2_app_kit::NSWorkspace;

    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .and_then(|app| app.bundleIdentifier())
        .map(|s| s.to_string().to_ascii_lowercase())
}

fn list_apps_impl(include_all: bool) -> Result<Vec<ListedApp>> {
    use std::time::Instant;
    let t0 = Instant::now();

    let frontmost = crate::platform::run_synthetic_input(frontmost_bundle_id);
    let t_running = Instant::now();

    let mut by_bundle: HashMap<String, ListedApp> = HashMap::new();
    if include_all {
        for app in installed_application_catalog()? {
            let key = app.identifier.to_ascii_lowercase();
            by_bundle.insert(key, app);
        }
    }

    let running = user_facing_running(&frontmost)?;
    let t_windows = Instant::now();
    for app in running {
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

    let t_catalog = Instant::now();
    let cutoff = recent_usage_cutoff_days(RECENT_USAGE_DAYS);
    let recent = spotlight_recent_apps(cutoff)?;
    let t_spotlight = Instant::now();
    for record in recent {
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
        "list_apps macOS: total={} running={} include_all={include_all} | \
         running {:?}, windows {:?}, catalog {:?}, spotlight {:?}, total {:?}",
        entries.len(),
        entries.iter().filter(|e| e.running).count(),
        t_running - t0,
        t_windows - t_running,
        t_catalog - t_windows,
        t_spotlight - t_catalog,
        t_spotlight - t0,
    );
    Ok(entries)
}

pub fn launch_app(app: &str, options: AppOpenOptions) -> Result<AppOpenResult> {
    if !macos_permissions::accessibility_effective() {
        return Err(anyhow!(
            "Accessibility permission required for launch_app on macOS. Enable Pointer in System Settings → Privacy & Security → Accessibility."
        ));
    }
    // 同 `list_apps`：整段上主线程会把界面一起冻住（实测 launch_app 9.7s、
    // list_apps 4.9s）。主线程只留给 NSWorkspace 那几次调用。
    launch_app_impl(app, options)
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

    // Nothing matched. Answer with the real app names instead of asking the
    // model for another guess — see `suggest_app_candidates`.
    Err(anyhow!("{}", super::launch_miss_message(app)))
}

fn user_facing_running(frontmost: &Option<String>) -> Result<Vec<ListedApp>> {
    // 主线程只做 NSWorkspace 的枚举（几毫秒）；`has_onscreen_window_for_pid`
    // 会去拉整张窗口列表，逐个应用各拉一次，放到主线程就是肉眼可见的卡顿。
    let rows = crate::platform::run_synthetic_input(running_app_rows);
    let mut out = Vec::with_capacity(rows.len());
    for (name, bundle, pid) in rows {
        let key = bundle.to_ascii_lowercase();
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
            pid: Some(pid as u32),
        });
    }
    Ok(out)
}

/// NSWorkspace 侧的运行中应用快照：`(名字, bundle id, pid)`。
fn running_app_rows() -> Vec<(String, String, i32)> {
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
        out.push((name, bundle, app.processIdentifier()));
    }
    out
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

/// 只要最近用过的应用：日期条件交给 mdfind。
///
/// 比「捞全部再逐个 mdls」少几个数量级：实测 `/Applications` 54 个应用里
/// 14 天内用过的只有 17 个。多取一天，最终仍由 `mdls_app_record` 按日粒度裁。
fn collect_recent_mdfind_app_paths(
    root: &Path,
    cutoff: NaiveDate,
    paths: &mut HashSet<PathBuf>,
) {
    let days = (chrono::Local::now().date_naive() - cutoff).num_days().max(0) + 1;
    let query = format!(
        "kMDItemContentType==com.apple.application-bundle && kMDItemLastUsedDate>=$time.now(-{days}d)"
    );
    let Ok(output) = Command::new("mdfind")
        .args(["-onlyin", &root.to_string_lossy(), query.as_str()])
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
    // 让 Spotlight 自己按日期筛，而不是「全部捞回来再逐个 mdls 过滤」——
    // 后者对每个装着的应用都起一次子进程。多要一天是为了让下面的
    // `mdls_app_record` 仍按原来的日粒度做最终判定，边界语义不变。
    collect_recent_mdfind_app_paths(root, cutoff, &mut paths);
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

/// 目录清单的每条记录。
///
/// **不跑 `mdls`**：`include_all` 只需要 bundle id 与显示名，Info.plist 里就有，
/// 而 `mdls` 是每个应用一次子进程 —— 263 个应用约 4.4 秒，是 `list_apps` 里最贵
/// 的一段。纯文件读同一个规模只要 70 毫秒（实测），而且没有「输出不带文件名、
/// 只能按行序对位」的解析风险。
///
/// 最近使用时间只对最近用过的应用有意义，那部分走 [`spotlight_apps_in`]。
fn app_catalog_record(path: &str) -> Option<ListedApp> {
    // 二进制 Info.plist（实测 263 个应用里 13 个，如 Adobe Acrobat、GarageBand）
    // 没法按文本找 `<key>`，直接交给 mdls；XML 的走文本解析。这是按格式分支，
    // 不是「失败重试」—— 两种编码各有各的读法。
    let Some(bytes) = bundle_plist_bytes(path) else {
        return mdls_catalog_record(path);
    };
    // 后台应用（LSUIElement / LSBackgroundOnly）直接排除。这一步必须在问
    // Spotlight 之前：`mdls` 自己也会再判一次后台应用然后返回 None，白跑
    // 一次 20ms 的子进程 —— CoreServices 里几乎全是这种。
    if is_background_bundle_from(&bytes) {
        return None;
    }
    if !is_binary_bundle_plist(&bytes) {
        if let Some(record) = plist_catalog_record_from(&bytes, path) {
            return Some(record);
        }
        // XML 但解析不出来（格式异于常规）：交给 Spotlight 兜住。
    }
    mdls_catalog_record(path)
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

/// Info.plist 是否二进制编码（`bplist00` 头）。二进制没法按 `<key>` 文本解析。
fn is_binary_bundle_plist(bytes: &[u8]) -> bool {
    bytes.starts_with(b"bplist00")
}

fn bundle_plist_bytes(path: &str) -> Option<Vec<u8>> {
    fs::read(PathBuf::from(path).join("Contents/Info.plist")).ok()
}

fn plist_catalog_record_from(bytes: &[u8], path: &str) -> Option<ListedApp> {
    let text = String::from_utf8_lossy(bytes).into_owned();
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

fn is_background_bundle_from(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
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

/// 从 `mdls` 输出里取一个属性值。
///
/// 属性名在 `=` 前**按最长名字补空格对齐**（`kMDItemUseCount           = 5`），
/// 所以不能拼 `"{key} = "` 去找 —— 只有最长那个属性恰好是单个空格，其余全都
/// 匹配不上，取值永远是 None。按行切 `=` 再比名字才稳。
fn mdls_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim() != key {
            continue;
        }
        let value = value.trim();
        if value == "(null)" {
            return None;
        }
        return Some(value.trim_matches('"').to_string());
    }
    None
}

/// 主线程上发出的激活请求结果。
enum ActivationAttempt {
    /// 激活请求已发出 —— 是否真的到了前台要在主线程之外等。
    Requested { regular: bool },
    /// Accessory（菜单栏）应用：无法抬到前台，但它在运行，按「已打开」处理。
    AccessoryRunning,
    /// 没有匹配的运行中实例。
    NotFound,
}

fn activate_running(app: &str) -> Result<bool> {
    use std::thread;
    use std::time::Duration;

    // 主线程只负责「发激活请求」，不 sleep、不跑子进程、不做轮询 ——
    // 那些都在下面，等在主线程上界面就卡住。
    let attempt = crate::platform::run_synthetic_input(|| request_activation(app));

    match attempt {
        ActivationAttempt::AccessoryRunning => {
            log::info!(
                "launch_app macOS activate: accessory app is running but not raisable app={app}"
            );
            return Ok(true);
        }
        ActivationAttempt::Requested { regular } => {
            thread::sleep(Duration::from_millis(ACTIVATE_SETTLE_MS));
            if running_app_state(app).is_some_and(|o| o.frontmost) {
                return Ok(true);
            }
            log::info!(
                "launch_app macOS activate: NSWorkspace activate did not frontmost app={app} regular={regular}"
            );
        }
        ActivationAttempt::NotFound => {}
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

/// 在**主线程**上枚举运行中实例并发出激活请求。
///
/// 只做 NSWorkspace 调用：先 Regular 再 Accessory 两遍 —— 有窗口的应用优先，
/// 菜单栏应用也算「已经开着」，否则会再开一份。
fn request_activation(app: &str) -> ActivationAttempt {
    use objc2_app_kit::{
        NSApplicationActivationOptions, NSApplicationActivationPolicy, NSWorkspace,
    };

    let workspace = NSWorkspace::sharedWorkspace();
    let apps = workspace.runningApplications();
    let count = apps.count();
    let mut requested: Option<bool> = None;
    for regular_pass in [true, false] {
        for i in 0..count {
            let running = apps.objectAtIndex(i);
            if running.isTerminated() {
                continue;
            }
            let is_regular = running.activationPolicy() == NSApplicationActivationPolicy::Regular;
            if is_regular != regular_pass {
                continue;
            }
            let localized = running.localizedName().map(|s| s.to_string());
            let bundle = running.bundleIdentifier().map(|s| s.to_string());
            if !app_identifier_matches(app, localized.as_deref(), bundle.as_deref()) {
                continue;
            }
            // `ActivateAllWindows` raises every window, not just the main one.
            // `ActivateIgnoringOtherApps` lets the activation win even when a
            // different app holds focus — without it `activateWithOptions`
            // routinely returns false and we fall through to `open -a`, which
            // is where a second copy can appear. The flag is deprecated on
            // macOS 14+ (where `activate` ignores other apps by default) but
            // still accepted, and older systems need it.
            #[allow(deprecated)]
            let opts = NSApplicationActivationOptions::ActivateAllWindows
                | NSApplicationActivationOptions::ActivateIgnoringOtherApps;
            if running.activateWithOptions(opts) {
                requested = Some(is_regular);
                break;
            }
            if !is_regular {
                return ActivationAttempt::AccessoryRunning;
            }
        }
        if requested.is_some() {
            break;
        }
    }
    match requested {
        Some(regular) => ActivationAttempt::Requested { regular },
        None => ActivationAttempt::NotFound,
    }
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
///
/// Accessory (menu-bar) apps are included: they still have a localized name,
/// and `activate_running` now treats them as running, so the message needs to
/// be able to name them too.
fn localized_display_name(app: &str) -> Option<String> {
    let app = app.to_string();
    crate::platform::run_synthetic_input(move || localized_display_name_on_main(&app))
}

fn localized_display_name_on_main(app: &str) -> Option<String> {
    use objc2_app_kit::NSWorkspace;

    let workspace = NSWorkspace::sharedWorkspace();
    let apps = workspace.runningApplications();
    let count = apps.count();
    for i in 0..count {
        let running = apps.objectAtIndex(i);
        if running.isTerminated() {
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
    let app = app.to_string();
    crate::platform::run_synthetic_input(move || running_app_state_on_main(&app))
}

fn running_app_state_on_main(app: &str) -> Option<LaunchVerifyOutcome> {
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
    let bundle = bundle.to_string();
    crate::platform::run_synthetic_input(move || application_url_for_bundle_on_main(&bundle))
}

fn application_url_for_bundle_on_main(bundle: &str) -> Option<PathBuf> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;

    let workspace = NSWorkspace::sharedWorkspace();
    let id = NSString::from_str(bundle);
    let url = workspace.URLForApplicationWithBundleIdentifier(&id)?;
    Some(PathBuf::from(url.path()?.to_string()))
}

fn app_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }
    roots
}

fn application_url_named(name: &str) -> Option<PathBuf> {
    let target = strip_app_suffix(name);
    for root in app_roots() {
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

#[cfg(test)]
mod catalog_tests {
    use super::*;

    const XML_PLIST: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.example.Demo</string>
<key>CFBundleName</key><string>Demo</string>
</dict></plist>"#;

    /// `mdls` 的属性名按最长名字补空格对齐，不能拼 `"{key} = "` 去找。
    /// 之前只有最长那个属性恰好对得上，`kMDItemLastUsedDate` / `kMDItemUseCount`
    /// 永远取不到 —— 「最近使用」列表因此恒为空。
    #[test]
    fn mdls_value_tolerates_padded_attribute_names() {
        let text = "kMDItemCFBundleIdentifier = \"com.apple.Safari\"\n\
kMDItemDisplayName        = \"Safari浏览器\"\n\
kMDItemLastUsedDate       = 2026-11-03 19:29:22 +0000\n\
kMDItemUseCount           = 5\n";
        assert_eq!(mdls_value(text, "kMDItemCFBundleIdentifier").as_deref(), Some("com.apple.Safari"));
        assert_eq!(mdls_value(text, "kMDItemLastUsedDate").as_deref(), Some("2026-11-03 19:29:22 +0000"));
        assert_eq!(mdls_value(text, "kMDItemUseCount").as_deref(), Some("5"));
        assert_eq!(mdls_value(text, "kMDItemMissing"), None);
    }

    /// XML plist 直接出记录，不需要 `mdls` 子进程。
    #[test]
    fn xml_plist_yields_record_without_mdls() {
        let rec = plist_catalog_record_from(XML_PLIST, "/nonexistent/Demo.app")
            .expect("XML plist 应该能解析出记录");
        assert_eq!(rec.identifier, "com.example.Demo");
        assert_eq!(rec.name, "Demo");
        assert!(rec.last_used.is_none(), "目录清单不需要使用时间");
    }

    /// 二进制 plist 必须被识别出来，走 `mdls` —— 按文本解析必然失败。
    /// 实测 263 个应用里 13 个是二进制（Adobe Acrobat、GarageBand 等），
    /// 少了这个分支它们会从清单里消失。
    #[test]
    fn binary_plist_is_detected_and_not_text_parsed() {
        let binary: &[u8] = b"bplist00\x00\x01\x02binary-payload";
        assert!(is_binary_bundle_plist(binary));
        assert!(!is_binary_bundle_plist(XML_PLIST));
        assert!(
            plist_catalog_record_from(binary, "/nonexistent/Demo.app").is_none(),
            "二进制内容不该被当文本解析出记录"
        );
    }
}

