//! Platform-specific integration helpers.

/// Run a closure that drives synthetic keyboard/mouse input (enigo).
///
/// On macOS, Text Input Source / keyboard layout APIs used by enigo are main-thread-only; calling
/// them from Tokio worker threads commonly crashes the host (SIGTRAP / immediate exit in Tauri).
#[inline]
pub fn run_synthetic_input<R: Send, F: FnOnce() -> R + Send>(f: F) -> R {
    // `cargo test` runs each case on its own thread and never drains the GCD
    // main queue, so `exec_sync` blocks forever there — the whole `state` module
    // hung on exactly that. Tests drive the backend without real key delivery,
    // so run inline; only the shipping binary needs the hop.
    #[cfg(all(target_os = "macos", not(test)))]
    {
        macos::run_on_main_thread_sync(f)
    }
    #[cfg(any(not(target_os = "macos"), test))]
    {
        f()
    }
}

/// Run `f` on the process main thread and return its value.
///
/// macOS AppKit objects (`NSPasteboard`, `NSWorkspace`, Text Input Source) may only be touched from
/// the main thread; the alternatives are a native abort or a silently wrong result. Off macOS the
/// closure runs inline — see [`clipboard`] for the crash this exists to prevent.
///
/// Runs inline when the caller is already on the main thread, so nesting is free.
pub fn run_on_main_thread<T, F>(f: F) -> anyhow::Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    #[cfg(target_os = "macos")]
    {
        macos::run_on_main_thread(f)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(f())
    }
}

pub mod app_access;
pub mod clipboard;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub mod macos_permissions;
