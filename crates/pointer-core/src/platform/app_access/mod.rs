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
