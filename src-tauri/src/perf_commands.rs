//! Frontend render-perf observations → the normal runtime log.
//!
//! The render-perf HUD lives in the WebView, so the one thing Rust cannot see —
//! a frame the main thread froze for a second — has to be handed over. These
//! lines go through `log::warn!` into the existing `pointer_*.log` channel (see
//! `pointer_core::logging`); there is no separate perf log file.
//!
//! The frontend only calls this while the HUD is enabled, and only for events
//! above the 300 ms thresholds, so the channel stays rare and bounded.

/// Write one already-formatted `perf_*` line at `warn` level.
///
/// Formatting stays in `src/lib/renderPerf.ts` (`perf_gap:` / `perf_input:`)
/// so the exact field contract is unit-tested there; this side only adds the
/// timestamped file/stderr line every other Rust log line gets.
#[tauri::command]
pub fn perf_log(msg: String) {
    log::warn!("{}", msg);
}
