//! Resolve a launched app's main window center for capture-monitor follow.

/// Capture monitor id for the frontmost app's primary visible window.
pub fn monitor_id_for_frontmost_app() -> Option<String> {
    #[cfg(windows)]
    {
        return super::windows::monitor_id_for_frontmost_app();
    }
    #[cfg(target_os = "macos")]
    {
        return super::macos::monitor_id_for_frontmost_app();
    }
    #[cfg(target_os = "linux")]
    {
        return super::linux::monitor_id_for_frontmost_app();
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// Capture monitor id for the target app's primary visible window.
pub fn monitor_id_for_launched_app(app: &str) -> Option<String> {
    #[cfg(windows)]
    {
        let (x, y) = super::windows::window_center_for_app(app)?;
        return crate::agents::computer::screen::monitor_id_at_global_point(x, y).ok();
    }
    #[cfg(target_os = "macos")]
    {
        return super::macos::monitor_id_for_launched_app(app);
    }
    #[cfg(target_os = "linux")]
    {
        let (x, y) = super::linux::window_center_for_app(app)?;
        return crate::agents::computer::screen::monitor_id_at_global_point(x, y).ok();
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = app;
        None
    }
}

/// Global screen point (top-left origin) of the target app's primary visible window.
pub fn window_center_for_app(app: &str) -> Option<(i32, i32)> {
    #[cfg(windows)]
    {
        return super::windows::window_center_for_app(app);
    }
    #[cfg(target_os = "macos")]
    {
        return super::macos::window_center_for_app(app);
    }
    #[cfg(target_os = "linux")]
    {
        return super::linux::window_center_for_app(app);
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = app;
        None
    }
}
