//! Bootstrap process `PATH` on Unix so GUI-launched hosts match login-shell tooling (npm, cargo, …).

use log::{info, warn};
use std::collections::HashSet;
use std::process::Command;

const PATH_VAR: &str = "PATH";

#[cfg(unix)]
const PATH_SEP: char = ':';

/// Merge login-shell paths first, then append any entries only present in `current`.
pub fn merge_path_entries(current: &str, login: &str) -> String {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for segment in login.split(PATH_SEP).chain(current.split(PATH_SEP)) {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        if seen.insert(segment.to_string()) {
            out.push(segment.to_string());
        }
    }
    out.join(&PATH_SEP.to_string())
}

/// On macOS/Linux, run the user's login shell once and merge its `PATH` into this process.
#[cfg(unix)]
pub fn bootstrap_process_path_from_login_shell() {
    let current = std::env::var(PATH_VAR).unwrap_or_default();
    let Some(login_path) = read_login_shell_path() else {
        warn!("shell_env: login-shell PATH unavailable; keeping process PATH");
        return;
    };
    if login_path.trim().is_empty() {
        warn!("shell_env: login-shell PATH empty; keeping process PATH");
        return;
    }
    let merged = merge_path_entries(&current, &login_path);
    if merged == current {
        info!("shell_env: process PATH already includes login-shell entries");
        return;
    }
    std::env::set_var(PATH_VAR, &merged);
    info!(
        "shell_env: merged login-shell PATH ({} → {} entries)",
        current.split(PATH_SEP).filter(|s| !s.is_empty()).count(),
        merged.split(PATH_SEP).filter(|s| !s.is_empty()).count()
    );
}

#[cfg(not(unix))]
pub fn bootstrap_process_path_from_login_shell() {}

#[cfg(unix)]
fn read_login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(default_login_shell);
    if !std::path::Path::new(&shell).exists() {
        warn!("shell_env: login shell not found: {shell}");
        return None;
    }
    let output = Command::new(&shell)
        .arg("-ilc")
        .arg("printf %s \"$PATH\"")
        .output()
        .map_err(|e| {
            warn!("shell_env: failed to run login shell {shell}: {e}");
            e
        })
        .ok()?;
    if !output.status.success() {
        warn!(
            "shell_env: login shell {shell} exited with {}",
            output.status
        );
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return None;
    }
    Some(path)
}

#[cfg(unix)]
fn default_login_shell() -> String {
    if cfg!(target_os = "macos") {
        "/bin/zsh".to_string()
    } else {
        "/bin/bash".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_path_deduplicates_and_prefers_login_order() {
        let merged = merge_path_entries("/usr/bin:/bin", "/opt/homebrew/bin:/usr/bin:/bin");
        assert_eq!(merged, "/opt/homebrew/bin:/usr/bin:/bin");
    }

    #[test]
    fn merge_path_keeps_current_only_entries() {
        let merged = merge_path_entries("/custom/bin:/usr/bin", "/usr/bin");
        assert_eq!(merged, "/usr/bin:/custom/bin");
    }
}
