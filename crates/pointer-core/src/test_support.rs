//! Test-only helpers shared across modules.
//!
//! The lib test binary runs every `#[cfg(test)]` module in **one process on many
//! threads**, so a test that mutates process-global state (env vars, the app data
//! dir, global settings) races every other test that reads the same global.
//! A lock scoped to a single `mod tests` cannot stop a *different* module from
//! clobbering the same variable — the locks below are deliberately shared.
//!
//! [`EnvRestore`] additionally restores the previous values on drop, so a failing
//! assertion cannot leak a mutated global into the tests that run after it.

use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};

/// Serializes tests that mutate **or read** `HOME` / `USERPROFILE`.
///
/// `HOME` decides tilde expansion (`~`), the `~/.agents/skills` scan and the
/// `dirs::home_dir()` / `dirs::data_dir()` fallbacks, so writers and readers
/// must take the same lock.
pub(crate) fn home_env_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Serializes tests that mutate **or read** `PATH`.
///
/// `PATH` decides whether `Command::new("git")` / `defaults` / `sh` resolve, so
/// a test that swaps it must not overlap with any test that spawns a process.
pub(crate) fn path_env_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
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

/// Restores the captured process env vars on drop (even when the test panics).
pub(crate) struct EnvRestore {
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl EnvRestore {
    /// Capture the current value of `keys`; call before mutating them.
    pub(crate) fn capture(keys: &[&'static str]) -> Self {
        Self {
            saved: keys.iter().map(|k| (*k, std::env::var_os(k))).collect(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}
