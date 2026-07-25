//! Bootstrap process `PATH` so GUI-launched hosts see the same tooling as a user shell.
//!
//! - Unix: merge login-shell `PATH` once at startup.
//! - Windows: merge registry User+Machine `Path` once at startup.

use log::{info, warn};
use std::collections::HashSet;
#[cfg(unix)]
use std::process::Command;

const PATH_VAR: &str = "PATH";

#[cfg(windows)]
const PATH_SEP: char = ';';
#[cfg(not(windows))]
const PATH_SEP: char = ':';

/// Merge `prepend` path segments first, then append any entries only present in `current`.
pub fn merge_path_entries(current: &str, prepend: &str) -> String {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for segment in prepend.split(PATH_SEP).chain(current.split(PATH_SEP)) {
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

/// Move Windows Store app-execution-alias dirs (e.g. `WindowsApps\python.exe`) to the end of `PATH`.
#[cfg(windows)]
pub fn demote_windows_app_execution_aliases(path: &str) -> String {
    let mut normal = Vec::new();
    let mut aliases = Vec::new();
    for segment in path.split(PATH_SEP) {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        if segment.to_ascii_lowercase().contains("windowsapps") {
            aliases.push(segment.to_string());
        } else {
            normal.push(segment.to_string());
        }
    }
    if aliases.is_empty() {
        return path.to_string();
    }
    normal.extend(aliases);
    normal.join(&PATH_SEP.to_string())
}

#[cfg(not(windows))]
pub fn demote_windows_app_execution_aliases(path: &str) -> String {
    path.to_string()
}

/// Refresh process `PATH` from the platform shell / registry snapshot.
pub fn bootstrap_process_path_from_login_shell() {
    #[cfg(unix)]
    bootstrap_unix_login_shell_path();
    #[cfg(windows)]
    merge_registry_path_into_process();
}

/// Windows: re-read User+Machine `Path` from the registry into this process (<1 ms).
#[cfg(windows)]
pub fn refresh_process_path_from_registry() {
    merge_registry_path_into_process();
}

#[cfg(not(windows))]
pub fn refresh_process_path_from_registry() {}

#[cfg(unix)]
fn bootstrap_unix_login_shell_path() {
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
    let merged = demote_windows_app_execution_aliases(&merged);
    std::env::set_var(PATH_VAR, &merged);
    info!(
        "shell_env: merged login-shell PATH ({} → {} entries)",
        current.split(PATH_SEP).filter(|s| !s.is_empty()).count(),
        merged.split(PATH_SEP).filter(|s| !s.is_empty()).count()
    );
}

#[cfg(windows)]
fn merge_registry_path_into_process() {
    let current = std::env::var(PATH_VAR)
        .or_else(|_| std::env::var("Path"))
        .unwrap_or_default();
    let Some(registry_path) = read_windows_registry_path() else {
        warn!("shell_env: registry PATH unavailable; keeping process PATH");
        return;
    };
    if registry_path.trim().is_empty() {
        warn!("shell_env: registry PATH empty; keeping process PATH");
        return;
    }
    let merged =
        demote_windows_app_execution_aliases(&merge_path_entries(&current, &registry_path));
    if merged == current {
        return;
    }
    std::env::set_var(PATH_VAR, &merged);
    info!(
        "shell_env: merged registry User+Machine PATH ({} → {} entries)",
        current.split(PATH_SEP).filter(|s| !s.is_empty()).count(),
        merged.split(PATH_SEP).filter(|s| !s.is_empty()).count()
    );
}

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

#[cfg(windows)]
fn read_windows_registry_path() -> Option<String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let machine = read_registry_path_value(
        &RegKey::predef(HKEY_LOCAL_MACHINE),
        r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        "Machine",
    );
    let user = read_registry_path_value(&RegKey::predef(HKEY_CURRENT_USER), "Environment", "User");

    match (machine, user) {
        (Some(m), Some(u)) if !m.is_empty() && !u.is_empty() => Some(format!("{m};{u}")),
        (Some(m), _) if !m.is_empty() => Some(m),
        (_, Some(u)) if !u.is_empty() => Some(u),
        _ => None,
    }
}

#[cfg(windows)]
fn read_registry_path_value(hive: &winreg::RegKey, subkey: &str, label: &str) -> Option<String> {
    let key = hive
        .open_subkey(subkey)
        .map_err(|e| {
            warn!("shell_env: failed to open registry {label} Path ({subkey}): {e}");
            e
        })
        .ok()?;
    key.get_value::<String, _>("Path")
        .map_err(|e| {
            warn!("shell_env: failed to read registry {label} Path: {e}");
            e
        })
        .ok()
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
    fn merge_path_deduplicates_and_prefers_prepend_order() {
        #[cfg(windows)]
        {
            let merged = merge_path_entries(
                r"C:\Windows\System32;C:\Windows",
                r"C:\Python314;C:\Windows\System32",
            );
            assert_eq!(merged, r"C:\Python314;C:\Windows\System32;C:\Windows");
        }
        #[cfg(not(windows))]
        {
            let merged = merge_path_entries("/usr/bin:/bin", "/opt/homebrew/bin:/usr/bin:/bin");
            assert_eq!(merged, "/opt/homebrew/bin:/usr/bin:/bin");
        }
    }

    #[test]
    #[cfg(windows)]
    fn demote_windows_app_execution_aliases_moves_stub_last() {
        let path =
            r"C:\Users\me\AppData\Local\Microsoft\WindowsApps;C:\Python314;C:\Windows\System32";
        let demoted = demote_windows_app_execution_aliases(path);
        assert_eq!(
            demoted,
            r"C:\Python314;C:\Windows\System32;C:\Users\me\AppData\Local\Microsoft\WindowsApps"
        );
    }

    #[test]
    fn merge_path_keeps_current_only_entries() {
        #[cfg(windows)]
        {
            let merged = merge_path_entries(r"C:\custom\bin;C:\Windows", r"C:\Windows");
            assert_eq!(merged, r"C:\Windows;C:\custom\bin");
        }
        #[cfg(not(windows))]
        {
            let merged = merge_path_entries("/custom/bin:/usr/bin", "/usr/bin");
            assert_eq!(merged, "/usr/bin:/custom/bin");
        }
    }
}
