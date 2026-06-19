//! Platform-specific integration helpers.

/// Run a closure that drives synthetic keyboard/mouse input (enigo).
///
/// On macOS, Text Input Source / keyboard layout APIs used by enigo are main-thread-only; calling
/// them from Tokio worker threads commonly crashes the host (SIGTRAP / immediate exit in Tauri).
#[inline]
pub fn run_synthetic_input<R: Send, F: FnOnce() -> R + Send>(f: F) -> R {
    #[cfg(target_os = "macos")]
    {
        macos::run_on_main_thread_sync(f)
    }
    #[cfg(not(target_os = "macos"))]
    {
        f()
    }
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub mod macos_permissions;
pub mod app_access;
