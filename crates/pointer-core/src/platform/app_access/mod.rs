//! Cross-platform application list/launch via OS accessibility (Codex Computer Use aligned).

mod launch_verify;
mod listed_app;
mod types;

#[cfg(any(target_os = "linux", test))]
mod linux_xbel;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod linux_recent;
#[cfg(target_os = "macos")]
mod macos;
mod window_monitor;
#[cfg(windows)]
mod windows;

pub use listed_app::{render_list, ListedApp};
pub use types::{AppOpenOptions, AppOpenResult, ListAppsOptions};
pub use window_monitor::{
    monitor_id_for_frontmost_app, monitor_id_for_launched_app, window_center_for_app,
};

/// List apps visible to the agent (Codex catalog).
/// Default: running apps plus 14-day recent usage; set `include_all` for the full installed catalog.
pub fn list_apps(options: ListAppsOptions) -> anyhow::Result<Vec<ListedApp>> {
    #[cfg(target_os = "macos")]
    {
        return macos::list_apps(options);
    }
    #[cfg(windows)]
    {
        return windows::list_apps(options);
    }
    #[cfg(target_os = "linux")]
    {
        return linux::list_apps(options);
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        Err(anyhow::anyhow!(
            "list_apps is not supported on this platform"
        ))
    }
}

/// App identifiers that look like `app`, for a failed `launch_app` reply.
///
/// The model only knows what the user said; installed bundle names are often
/// longer, localized, or vendor-prefixed. A failed launch answers with the real
/// names instead of asking for another blind guess (each retry also pays the
/// launch-verification poll).
///
/// Matching is a case-insensitive substring test in both directions, so a short
/// spoken name finds a long bundle name and vice versa. Shortest first — it is
/// the likeliest exact intent.
pub fn suggest_app_candidates(app: &str, limit: usize) -> Vec<String> {
    let Ok(apps) = list_apps(ListAppsOptions { include_all: true }) else {
        return Vec::new();
    };
    rank_app_candidates(&apps, app, limit)
}

/// Pure ranking half of [`suggest_app_candidates`], so it can be tested without
/// touching the OS catalog.
pub fn rank_app_candidates(apps: &[ListedApp], app: &str, limit: usize) -> Vec<String> {
    let needle = app
        .trim()
        .strip_suffix(".app")
        .unwrap_or(app.trim())
        .to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<String> = Vec::new();
    for entry in apps {
        let hay = entry.name.to_lowercase();
        let hay_id = entry.identifier.to_lowercase();
        let hit = hay == needle
            || hay.contains(&needle)
            || needle.contains(&hay)
            || hay_id == needle
            || hay_id.contains(&needle);
        if hit {
            found.push(entry.name.clone());
        }
    }
    found.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    found.dedup();
    found.truncate(limit);
    found
}

/// `launch_app` error text for a name that matched nothing exactly.
pub(crate) fn launch_miss_message(app: &str) -> String {
    let candidates = suggest_app_candidates(app, 5);
    if candidates.is_empty() {
        return format!(
            "Could not launch \"{app}\". No installed app matches that name. Call list_apps to see what is available."
        );
    }
    format!(
        "Could not launch \"{app}\" — no app is named exactly that. Did you mean: {}? Pass one of those names to launch_app as-is.",
        candidates.join(", ")
    )
}

/// Launch or activate an application by display name or bundle / executable identifier.
///
/// Default: activate a running instance when found, otherwise launch.
/// Set `new_instance: true` only when the user explicitly wants another window/process.
pub fn launch_app(app: &str, options: AppOpenOptions) -> anyhow::Result<AppOpenResult> {
    options.validate().map_err(anyhow::Error::msg)?;
    #[cfg(target_os = "macos")]
    {
        return macos::launch_app(app, options);
    }
    #[cfg(windows)]
    {
        return windows::launch_app(app, options);
    }
    #[cfg(target_os = "linux")]
    {
        return linux::launch_app(app, options);
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = app;
        Err(anyhow::anyhow!(
            "launch_app is not supported on this platform"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, identifier: &str) -> ListedApp {
        ListedApp {
            name: name.to_string(),
            identifier: identifier.to_string(),
            running: false,
            frontmost: false,
            last_used: None,
            uses: None,
            window_title: None,
            pid: None,
        }
    }

    #[test]
    fn rank_matches_a_short_name_inside_a_long_one() {
        let apps = vec![
            app("Numbers", "com.apple.Numbers"),
            app("Pages", "com.apple.Pages"),
        ];
        assert_eq!(rank_app_candidates(&apps, "Num", 5), vec!["Numbers"]);
        assert_eq!(
            rank_app_candidates(&apps, "num", 5),
            vec!["Numbers"],
            "matching must be case-insensitive"
        );
    }

    #[test]
    fn rank_matches_when_the_input_is_longer_than_the_name() {
        let apps = vec![app("Terminal", "com.apple.Terminal")];
        assert_eq!(rank_app_candidates(&apps, "Terminal.app", 5), vec!["Terminal"]);
    }

    #[test]
    fn rank_matches_on_the_identifier_too() {
        let apps = vec![app("Notes", "com.apple.Notes")];
        assert_eq!(rank_app_candidates(&apps, "com.apple.notes", 5), vec!["Notes"]);
    }

    #[test]
    fn rank_puts_the_shortest_name_first() {
        // Substring matching is deliberately loose; extras are fine because the
        // model picks from the list. The likeliest intent must lead.
        let apps = vec![
            app("Preview", "com.apple.Preview"),
            app("Preview Helper", "com.apple.PreviewHelper"),
        ];
        let out = rank_app_candidates(&apps, "Preview", 5);
        assert_eq!(out.first().map(String::as_str), Some("Preview"), "{out:?}");
        assert!(out.contains(&"Preview Helper".to_string()), "{out:?}");
    }

    #[test]
    fn rank_is_empty_when_nothing_matches_or_the_input_is_blank() {
        let apps = vec![app("Terminal", "com.apple.Terminal")];
        assert!(rank_app_candidates(&apps, "Zzz", 5).is_empty());
        assert!(
            rank_app_candidates(&apps, "   ", 5).is_empty(),
            "blank input must not match everything"
        );
    }

    #[test]
    fn rank_honours_the_limit() {
        let apps = vec![
            app("A1", "a.a1"),
            app("A2", "a.a2"),
            app("A3", "a.a3"),
            app("A4", "a.a4"),
        ];
        assert_eq!(rank_app_candidates(&apps, "A", 2).len(), 2);
    }
}
