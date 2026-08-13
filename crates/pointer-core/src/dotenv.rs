//! Parse `.env` files and apply variables to **terminal child processes**.
//!
//! Non-`PATH` keys from `.env` override inherited values for the child only (host unchanged).
//! `PATH` is **prepended** ahead of the inherited process `PATH` (Windows `;`, Unix `:`).

use anyhow::{anyhow, Result};
use log::{info, warn};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Parse a dotenv file into key/value pairs. Later duplicate keys in the same file win.
pub fn parse_dotenv_bytes(content: &[u8]) -> HashMap<String, String> {
    let content = strip_utf8_bom(content);
    let text = String::from_utf8_lossy(content);
    let mut out = HashMap::new();
    for line in text.lines() {
        let Some((key, value)) = parse_dotenv_line(line) else {
            continue;
        };
        out.insert(key, value);
    }
    out
}

/// Load one or more `.env` files (in order) into a merged map for child processes.
pub fn merged_env_from_files(env_files: &[PathBuf]) -> HashMap<String, String> {
    let mut merged: HashMap<String, String> = HashMap::new();
    for path in env_files {
        match std::fs::read(path) {
            Ok(bytes) => {
                for (k, v) in parse_dotenv_bytes(&bytes) {
                    let applied = env_value_for_child(&k, &v);
                    merged.insert(k, applied);
                }
            }
            Err(e) => {
                warn!("dotenv: failed to read {}: {e}", path.display());
            }
        }
    }
    merged
}

/// Full child environment for the `terminal` tool: Pointer process env + `.env` overlays
/// + session vars + settings `terminalEnvOverrides` (overrides win last).
///
/// Platform API keys and other sensitive variables are stripped so Skill scripts cannot
/// read them via `printenv`.
pub fn build_terminal_child_environment(env_files: &[PathBuf]) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = std::env::vars()
        .filter(|(k, _)| !is_sensitive_env_key(k))
        .collect();
    for path in env_files {
        match std::fs::read(path) {
            Ok(bytes) => {
                info!("dotenv: loaded {}", path.display());
                for (k, v) in parse_dotenv_bytes(&bytes) {
                    if is_sensitive_env_key(&k) {
                        warn!(
                            "dotenv: skipping sensitive key from {}: {k}",
                            path.display()
                        );
                        continue;
                    }
                    let applied = env_value_for_child(&k, &v);
                    env.insert(k, applied);
                }
            }
            Err(e) => {
                warn!("dotenv: failed to read {}: {e}", path.display());
            }
        }
    }
    #[cfg(windows)]
    crate::windows_shell_encoding::apply_windows_utf8_child_env(&mut env);
    crate::session_user_env::apply_session_user_id(&mut env);
    crate::session_work_dir_env::apply_session_work_dir(&mut env);
    crate::storage::apply_data_dir_env(&mut env);
    crate::skills::external::apply_skill_dir_env(&mut env);
    apply_terminal_env_overrides(&mut env);
    // After overrides: Dock-launched hosts often have LANG=C; PTY `ls` then prints `?`.
    #[cfg(unix)]
    crate::unix_locale::apply_unix_utf8_child_env(&mut env);
    env
}

/// Apply settings `terminalEnvOverrides` last (after process, `.env`, and session vars).
/// Independent of debug menus: closing debug keeps injection; values stay session-memory only.
fn apply_terminal_env_overrides(env: &mut HashMap<String, String>) {
    let settings = crate::platform_config::effective_settings_global();
    let overrides = settings.terminal_env_overrides;
    if overrides.is_empty() {
        return;
    }
    let mut applied = 0usize;
    for (k, v) in overrides {
        let key = k.trim();
        if key.is_empty() || !is_valid_env_key(key) {
            warn!("terminalEnvOverrides: skipping invalid key {k:?}");
            continue;
        }
        if is_sensitive_env_key(key) {
            warn!("terminalEnvOverrides: skipping sensitive key {key}");
            continue;
        }
        let applied_value = env_value_for_child(key, v.trim());
        env.insert(key.to_string(), applied_value);
        applied += 1;
    }
    if applied > 0 {
        info!("terminalEnvOverrides: applied {applied} key(s) to terminal child env");
    }
}

/// Keys withheld from terminal child processes (substring match, case-insensitive).
pub fn is_sensitive_env_key(key: &str) -> bool {
    const MARKERS: &[&str] = &[
        "API_KEY",
        "APIKEY",
        "SECRET",
        "TOKEN",
        "PASSWORD",
        "PRIVATE_KEY",
        "ACCESS_KEY",
        "DASHSCOPE",
        "OPENAI",
        "ANTHROPIC",
    ];
    let upper = key.trim().to_ascii_uppercase();
    MARKERS.iter().any(|m| upper.contains(m))
}

/// Load one or more `.env` files (in order) and apply them to a child `Command`.
pub fn apply_supplemental_env_files(cmd: &mut Command, env_files: &[PathBuf]) -> Vec<String> {
    let child_env = build_terminal_child_environment(env_files);
    let loaded: Vec<String> = env_files
        .iter()
        .filter(|p| p.is_file())
        .map(|p| p.display().to_string())
        .collect();
    cmd.env_clear();
    cmd.envs(child_env);
    loaded
}

/// Resolve the value to set on a child process for one `.env` entry.
pub fn env_value_for_child(key: &str, value: &str) -> String {
    if key.eq_ignore_ascii_case("PATH") {
        let inherited = inherited_process_path();
        let expanded = expand_path_value_placeholders(value, &inherited);
        let merged = crate::shell_env::merge_path_entries(&inherited, &expanded);
        return crate::shell_env::demote_windows_app_execution_aliases(&merged);
    }
    value.to_string()
}

fn inherited_process_path() -> String {
    std::env::var("PATH")
        .or_else(|_| std::env::var("Path"))
        .unwrap_or_default()
}

fn expand_path_value_placeholders(value: &str, inherited_path: &str) -> String {
    let mut out = value.replace("$PATH", inherited_path);
    for pattern in ["%PATH%", "%Path%", "%path%", "%PATH", "%Path", "%path"] {
        if out.contains(pattern) {
            out = out.replace(pattern, inherited_path);
        }
    }
    out
}

fn strip_utf8_bom(content: &[u8]) -> &[u8] {
    content.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(content)
}

/// Default user env file under the app data directory (`PointerApp/.env`).
pub fn default_user_env_file() -> Option<PathBuf> {
    match crate::storage::user_env_file_path() {
        Ok(path) if path.is_file() => Some(path),
        Ok(_) => None,
        Err(e) => {
            warn!("dotenv: app data dir unavailable for default .env: {e}");
            None
        }
    }
}

/// Resolve env file paths from tool args (`envFiles` string or array of strings).
pub fn parse_env_file_args(args: &serde_json::Value) -> Vec<String> {
    let Some(value) = args.get("envFiles") else {
        return vec![];
    };
    match value {
        serde_json::Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                vec![]
            } else {
                vec![s.to_string()]
            }
        }
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|item| item.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        _ => vec![],
    }
}

/// Resolve a user-supplied env file path against `cwd` (or workspace root fallback).
pub fn resolve_env_file_path(
    raw: &str,
    cwd: Option<&Path>,
    workspace_root: Option<&Path>,
) -> Result<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(anyhow!("env file path is empty"));
    }
    let path = Path::new(raw);
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let base = cwd
            .or(workspace_root)
            .ok_or_else(|| anyhow!("cannot resolve relative env file path (no cwd): {raw}"))?;
        base.join(path)
    };
    if !full.is_file() {
        return Err(anyhow!("env file not found: {raw}"));
    }
    Ok(full.canonicalize().unwrap_or(full))
}

fn parse_dotenv_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let line = line.strip_prefix("export ").unwrap_or(line).trim();
    let (key_part, value_part) = split_key_value(line)?;
    let key = key_part.trim();
    if key.is_empty() || !is_valid_env_key(key) {
        return None;
    }
    let value = parse_dotenv_value(value_part.trim());
    Some((key.to_string(), value))
}

fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let mut in_single = false;
    let mut in_double = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '=' if !in_single && !in_double => {
                return Some((&line[..i], &line[i + 1..]));
            }
            _ => {}
        }
    }
    None
}

fn parse_dotenv_value(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    if (raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2)
        || (raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2)
    {
        return unescape_quoted(&raw[1..raw.len() - 1], raw.starts_with('"'));
    }
    raw.to_string()
}

fn unescape_quoted(inner: &str, double: bool) -> String {
    if !double {
        return inner.to_string();
    }
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_test_guard() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn parse_basic_and_export() {
        let content = b"# comment\nFOO=bar\nexport BAZ=qux\nEMPTY=\n";
        let m = parse_dotenv_bytes(content);
        assert_eq!(m.get("FOO").map(|s| s.as_str()), Some("bar"));
        assert_eq!(m.get("BAZ").map(|s| s.as_str()), Some("qux"));
        assert_eq!(m.get("EMPTY").map(|s| s.as_str()), Some(""));
        assert!(!m.contains_key("# comment"));
    }

    #[test]
    fn parse_quoted_values() {
        let content = b"DOUBLE=\"hello world\"\nSINGLE='a=b'\nESC=\"line\\n2\"\n";
        let m = parse_dotenv_bytes(content);
        assert_eq!(m.get("DOUBLE").map(|s| s.as_str()), Some("hello world"));
        assert_eq!(m.get("SINGLE").map(|s| s.as_str()), Some("a=b"));
        assert_eq!(m.get("ESC").map(|s| s.as_str()), Some("line\n2"));
    }

    #[test]
    fn dotenv_overrides_non_path_for_child() {
        let _guard = env_test_guard();
        let key = "POINTER_DOTENV_TEST_ONLY";
        std::env::set_var(key, "from_process");
        assert_eq!(
            env_value_for_child(key, "from_file"),
            "from_file",
            "child should use .env value even when host defines the key"
        );
        std::env::remove_var(key);
    }

    #[test]
    fn dotenv_path_prepends_before_inherited() {
        let _guard = env_test_guard();
        #[cfg(windows)]
        {
            std::env::set_var("PATH", r"C:\Windows\System32");
            assert_eq!(
                env_value_for_child("PATH", r"C:\Python314"),
                r"C:\Python314;C:\Windows\System32"
            );
        }
        #[cfg(not(windows))]
        {
            std::env::set_var("PATH", "/usr/bin");
            assert_eq!(
                env_value_for_child("PATH", "/opt/python/bin"),
                "/opt/python/bin:/usr/bin"
            );
        }
    }

    #[test]
    fn dotenv_path_expands_percent_path_placeholder() {
        let _guard = env_test_guard();
        std::env::set_var("PATH", "/usr/bin");
        assert_eq!(
            env_value_for_child("PATH", "/opt/python:%PATH%"),
            "/opt/python:/usr/bin"
        );
        assert_eq!(
            env_value_for_child("PATH", "/opt/python:%PATH"),
            "/opt/python:/usr/bin"
        );
    }

    #[test]
    fn parse_strips_utf8_bom() {
        let content = b"\xEF\xBB\xBFFOO=bar\n";
        let m = parse_dotenv_bytes(content);
        assert_eq!(m.get("FOO").map(|s| s.as_str()), Some("bar"));
    }

    #[test]
    fn parse_env_file_args_accepts_array() {
        let args = serde_json::json!({
            "envFiles": [".env", ".env.local", "secrets.env"]
        });
        let paths = parse_env_file_args(&args);
        assert_eq!(paths, vec![".env", ".env.local", "secrets.env"]);
    }

    #[test]
    fn parse_env_file_args_accepts_single_string() {
        let args = serde_json::json!({ "envFiles": ".env" });
        assert_eq!(parse_env_file_args(&args), vec![".env"]);
    }

    #[test]
    fn build_terminal_child_environment_injects_session_user_id() {
        let _guard = env_test_guard();
        crate::platform_config::replace_global_platform_config_for_test(
            crate::models::PlatformSettings::default(),
        );
        let _user_guard = crate::session_user_env::SessionUserIdGuard::enter("user-42".into());
        let map = build_terminal_child_environment(&[]);
        assert_eq!(
            map.get("SESSION_USER_ID").map(String::as_str),
            Some("user-42")
        );
    }

    #[test]
    fn build_terminal_child_environment_injects_data_dir() {
        let _guard = env_test_guard();
        crate::platform_config::replace_global_platform_config_for_test(
            crate::models::PlatformSettings::default(),
        );
        let map = build_terminal_child_environment(&[]);
        let data_dir = map.get("DATA_DIR").expect("DATA_DIR");
        assert!(!data_dir.trim().is_empty());
        let expected = crate::storage::app_data_dir()
            .expect("app_data_dir")
            .to_string_lossy()
            .into_owned();
        assert_eq!(data_dir, &expected);
    }

    #[test]
    fn build_terminal_child_environment_injects_skill_dir() {
        let _guard = env_test_guard();
        crate::platform_config::replace_global_platform_config_for_test(
            crate::models::PlatformSettings::default(),
        );
        let map = build_terminal_child_environment(&[]);
        let skill_dir = map.get("SKILL_DIR").expect("SKILL_DIR");
        assert!(!skill_dir.trim().is_empty());
        let expected = crate::skills::external::pointer_skills_dir()
            .expect("pointer_skills_dir")
            .to_string_lossy()
            .into_owned();
        assert_eq!(skill_dir, &expected);
    }

    #[test]
    fn build_terminal_child_environment_injects_work_dir() {
        let _guard = env_test_guard();
        crate::platform_config::replace_global_platform_config_for_test(
            crate::models::PlatformSettings::default(),
        );
        let _dir_guard =
            crate::session_work_dir_env::SessionWorkDirGuard::enter("/tmp/pointer-ws".into());
        let map = build_terminal_child_environment(&[]);
        assert_eq!(
            map.get("WORKING_DIR").map(String::as_str),
            Some("/tmp/pointer-ws")
        );
    }

    #[test]
    fn build_terminal_child_environment_injects_session_context() {
        let _guard = env_test_guard();
        crate::platform_config::replace_global_platform_config_for_test(
            crate::models::PlatformSettings::default(),
        );
        let _user_guard = crate::session_user_env::SessionUserIdGuard::enter("user-42".into());
        let _dir_guard =
            crate::session_work_dir_env::SessionWorkDirGuard::enter("/tmp/pointer-ws".into());
        let map = build_terminal_child_environment(&[]);
        assert_eq!(
            map.get("SESSION_USER_ID").map(String::as_str),
            Some("user-42")
        );
        assert_eq!(
            map.get("WORKING_DIR").map(String::as_str),
            Some("/tmp/pointer-ws")
        );
    }

    #[test]
    fn build_terminal_child_environment_overlays_dotenv_on_process() {
        let _guard = env_test_guard();
        let dir = tempfile::tempdir().unwrap();
        let env_path = dir.path().join(".env");
        std::fs::write(&env_path, "POINTER_DOTENV_TEST_ONLY=from_file\n").unwrap();
        std::env::set_var("POINTER_DOTENV_TEST_ONLY", "from_process");
        std::env::set_var("POINTER_DOTENV_PROCESS_ONLY", "keep");
        let map = build_terminal_child_environment(&[env_path]);
        assert_eq!(
            map.get("POINTER_DOTENV_TEST_ONLY").map(|s| s.as_str()),
            Some("from_file")
        );
        assert_eq!(
            map.get("POINTER_DOTENV_PROCESS_ONLY").map(|s| s.as_str()),
            Some("keep")
        );
        std::env::remove_var("POINTER_DOTENV_TEST_ONLY");
        std::env::remove_var("POINTER_DOTENV_PROCESS_ONLY");
    }

    #[test]
    fn build_terminal_child_environment_applies_terminal_env_overrides() {
        let _guard = env_test_guard();
        std::env::set_var("POINTER_OVERRIDE_BASE", "from_process");
        let mut u = crate::models::UserSettings::default();
        u.debug_menus_enabled = false;
        u.terminal_env_overrides.insert(
            "POINTER_OVERRIDE_BASE".into(),
            "from_settings".into(),
        );
        u.terminal_env_overrides
            .insert("POINTER_OVERRIDE_NEW".into(), "added".into());
        crate::platform_config::replace_global_user_settings_for_test(u);
        let map = build_terminal_child_environment(&[]);
        assert_eq!(
            map.get("POINTER_OVERRIDE_BASE").map(String::as_str),
            Some("from_settings")
        );
        assert_eq!(
            map.get("POINTER_OVERRIDE_NEW").map(String::as_str),
            Some("added")
        );
        std::env::remove_var("POINTER_OVERRIDE_BASE");
        crate::platform_config::replace_global_user_settings_for_test(
            crate::models::UserSettings::default(),
        );
    }

    #[test]
    fn build_terminal_child_environment_overrides_session_vars() {
        let _guard = env_test_guard();
        let _uid = crate::session_user_env::SessionUserIdGuard::enter("session-uid".into());
        let _wd = crate::session_work_dir_env::SessionWorkDirGuard::enter("/session/work".into());
        let mut u = crate::models::UserSettings::default();
        u.debug_menus_enabled = false;
        u.terminal_env_overrides
            .insert("SESSION_USER_ID".into(), "override-uid".into());
        u.terminal_env_overrides
            .insert("WORKING_DIR".into(), "/override/work".into());
        crate::platform_config::replace_global_user_settings_for_test(u);
        let map = build_terminal_child_environment(&[]);
        assert_eq!(
            map.get("SESSION_USER_ID").map(String::as_str),
            Some("override-uid")
        );
        assert_eq!(
            map.get("WORKING_DIR").map(String::as_str),
            Some("/override/work")
        );
        crate::platform_config::replace_global_user_settings_for_test(
            crate::models::UserSettings::default(),
        );
    }

    #[test]
    fn build_terminal_child_environment_applies_overrides_when_debug_off() {
        let _guard = env_test_guard();
        let mut u = crate::models::UserSettings::default();
        u.debug_menus_enabled = false;
        u.terminal_env_overrides
            .insert("POINTER_OVERRIDE_NEW".into(), "added".into());
        crate::platform_config::replace_global_user_settings_for_test(u);
        let map = build_terminal_child_environment(&[]);
        assert_eq!(
            map.get("POINTER_OVERRIDE_NEW").map(String::as_str),
            Some("added")
        );
        crate::platform_config::replace_global_user_settings_for_test(
            crate::models::UserSettings::default(),
        );
    }

    #[test]
    fn parse_env_file_args_empty_when_omitted() {
        let args = serde_json::json!({ "command": "echo hi" });
        assert!(parse_env_file_args(&args).is_empty());
    }

    #[test]
    fn sensitive_env_keys_filtered() {
        assert!(is_sensitive_env_key("OPENAI_API_KEY"));
        assert!(is_sensitive_env_key("DASHSCOPE_API_KEY"));
        assert!(!is_sensitive_env_key("PATH"));
        assert!(!is_sensitive_env_key("HOME"));
    }
}
