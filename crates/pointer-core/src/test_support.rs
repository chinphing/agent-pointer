//! Test-only helpers shared across modules.
//!
//! The lib test binary runs every `#[cfg(test)]` module in **one process on many
//! threads**, so a test that mutates process-global state (env vars, the app data
//! dir, global settings) races every other test that reads the same global.
//! A lock scoped to a single `mod tests` cannot stop a *different* module from
//! clobbering the same variable — the locks below are deliberately shared.
//!
//! Roots (home, app data) are injected instead of mutated wherever a test needs
//! isolation — see `storage::TestPointerHome` / `set_test_app_data_dir`.

use std::sync::{Mutex, MutexGuard};

/// Serializes tests that mutate **or read** `PATH`.
///
/// `PATH` decides whether `Command::new("git")` / `defaults` / `sh` resolve, so
/// a test that swaps it must not overlap with any test that spawns a process.
pub(crate) fn path_env_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Build a **usable** marker `PATH` for tests that swap it process-wide.
///
/// The marker is prepended so the test still recognises its own value, while the
/// inherited entries are preserved. The lib test binary runs every `#[cfg(test)]`
/// module in one process, so a bare marker (no `/usr/bin`) makes a concurrent test
/// that spawns `git` / `python3` / `sh` fail to resolve the binary — a rare,
/// load-dependent flake that no single `mod tests` can guard against.
pub(crate) fn marker_path(marker: &str) -> String {
    let separator = if cfg!(windows) { ';' } else { ':' };
    match std::env::var("PATH") {
        Ok(existing) if !existing.trim().is_empty() => format!("{marker}{separator}{existing}"),
        _ => marker.to_string(),
    }
}

/// Serializes tests that touch the process-global remembered model context
/// windows (`context_compression::clear_remembered_model_windows_for_test`).
///
/// The map is keyed by provider+model, but a `clear()` from a concurrent test
/// wipes entries another test just inserted, which silently falls back to the
/// configured budget.
pub(crate) fn remembered_model_window_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

