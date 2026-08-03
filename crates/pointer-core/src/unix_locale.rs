//! Ensure terminal child processes get a UTF-8 locale on Unix.
//!
//! Dock/Finder-launched GUI apps often inherit `LANG=C` / empty locale. In a
//! real PTY, macOS `ls` then replaces non-ASCII filename bytes with `?` even
//! though the filesystem names are UTF-8. Windows has a parallel helper in
//! `windows_shell_encoding`.

#![cfg_attr(windows, allow(dead_code))]

use std::collections::HashMap;
use std::sync::OnceLock;

/// Apply a UTF-8 `LANG` / fix non-UTF-8 `LC_*` for shell children.
pub fn apply_unix_utf8_child_env(env: &mut HashMap<String, String>) {
    let lang_ok = env.get("LANG").is_some_and(|v| locale_value_is_utf8(v));
    let lc_all = env.get("LC_ALL").cloned();
    let lc_ctype = env.get("LC_CTYPE").cloned();

    // LC_ALL overrides everything — must be UTF-8 when present.
    if let Some(ref v) = lc_all {
        if !v.is_empty() && !locale_value_is_utf8(v) {
            let fixed = preferred_utf8_locale(env);
            log::info!(
                "unix_locale: replacing non-UTF-8 LC_ALL={v:?} with {fixed:?} for terminal child"
            );
            env.insert("LC_ALL".into(), fixed);
        }
    }

    if let Some(ref v) = lc_ctype {
        if !v.is_empty() && !locale_value_is_utf8(v) {
            let fixed = preferred_utf8_locale(env);
            log::info!(
                "unix_locale: replacing non-UTF-8 LC_CTYPE={v:?} with {fixed:?} for terminal child"
            );
            env.insert("LC_CTYPE".into(), fixed);
        }
    }

    if !lang_ok {
        let fixed = preferred_utf8_locale(env);
        log::info!("unix_locale: setting LANG={fixed:?} for terminal child (was missing/non-UTF-8)");
        env.insert("LANG".into(), fixed);
    }
}

fn locale_value_is_utf8(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    if lower.is_empty() || lower == "c" || lower == "posix" {
        return false;
    }
    lower.contains("utf-8") || lower.contains("utf8")
}

fn preferred_utf8_locale(env: &HashMap<String, String>) -> String {
    // Keep an existing UTF-8 preference from the parent when present.
    for key in ["LANG", "LC_ALL", "LC_CTYPE"] {
        if let Some(v) = env.get(key) {
            if locale_value_is_utf8(v) {
                return v.clone();
            }
        }
    }
    cached_system_utf8_locale().clone()
}

fn cached_system_utf8_locale() -> &'static String {
    static LOCALE: OnceLock<String> = OnceLock::new();
    LOCALE.get_or_init(detect_system_utf8_locale)
}

fn detect_system_utf8_locale() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Some(loc) = macos_apple_locale_utf8() {
            return loc;
        }
    }
    for candidate in [
        "en_US.UTF-8",
        "C.UTF-8",
        "UTF-8",
        "zh_CN.UTF-8",
        "zh_TW.UTF-8",
    ] {
        if locale_is_available(candidate) {
            return candidate.to_string();
        }
    }
    "en_US.UTF-8".to_string()
}

#[cfg(target_os = "macos")]
fn macos_apple_locale_utf8() -> Option<String> {
    let output = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLocale"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let apple = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if apple.is_empty() {
        return None;
    }
    // AppleLocale is like "zh_CN" or "en_US@currency=USD" — take the language_REGION prefix.
    let base = apple.split('@').next().unwrap_or(&apple).trim();
    if base.is_empty() {
        return None;
    }
    let candidate = format!("{base}.UTF-8");
    if locale_is_available(&candidate) {
        log::info!("unix_locale: using macOS AppleLocale → {candidate}");
        return Some(candidate);
    }
    // zh-Hans_CN style → zh_CN.UTF-8
    let normalized = base.replace('-', "_");
    let candidate = format!("{normalized}.UTF-8");
    if locale_is_available(&candidate) {
        return Some(candidate);
    }
    None
}

fn locale_is_available(name: &str) -> bool {
    let output = match std::process::Command::new("locale").arg("-a").output() {
        Ok(o) if o.status.success() => o,
        Ok(_) | Err(_) => return true, // assume available if we cannot probe
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let target = name.to_ascii_lowercase();
    text.lines().any(|line| line.trim().eq_ignore_ascii_case(&target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_value_is_utf8_detects_common_forms() {
        assert!(locale_value_is_utf8("en_US.UTF-8"));
        assert!(locale_value_is_utf8("zh_CN.utf8"));
        assert!(!locale_value_is_utf8("C"));
        assert!(!locale_value_is_utf8("POSIX"));
        assert!(!locale_value_is_utf8(""));
    }

    #[test]
    fn apply_sets_lang_when_missing() {
        let mut env = HashMap::new();
        apply_unix_utf8_child_env(&mut env);
        let lang = env.get("LANG").expect("LANG");
        assert!(locale_value_is_utf8(lang), "LANG={lang}");
    }

    #[test]
    fn apply_replaces_c_locale() {
        let mut env = HashMap::new();
        env.insert("LANG".into(), "C".into());
        env.insert("LC_ALL".into(), "C".into());
        apply_unix_utf8_child_env(&mut env);
        assert!(locale_value_is_utf8(env.get("LANG").unwrap()));
        assert!(locale_value_is_utf8(env.get("LC_ALL").unwrap()));
    }

    #[test]
    fn apply_preserves_existing_utf8_lang() {
        let mut env = HashMap::new();
        env.insert("LANG".into(), "zh_CN.UTF-8".into());
        apply_unix_utf8_child_env(&mut env);
        assert_eq!(env.get("LANG").map(String::as_str), Some("zh_CN.UTF-8"));
    }
}
