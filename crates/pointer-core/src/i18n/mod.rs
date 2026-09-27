//! UI message catalogs and host injects for strings rendered in `pointer-core`.
//!
//! Locale preference comes from [`crate::models::UserSettings::ui_locale`] (`system` | `zh-CN` | `en`),
//! resolved with the same rules as `src/lib/uiLocale.ts`. Catalog strings are rendered to final
//! locale text before they reach the frontend (tool labels, agent names, UiToast, etc.).

mod catalogs;

use std::sync::OnceLock;

/// Preference values persisted in `user_settings.json` (`uiLocale`).
pub type UiLocalePreference = str;

/// Resolved BCP-47-style UI locale used for catalog lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiLocale {
    ZhCn,
    En,
}

impl UiLocale {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::En => "en",
        }
    }
}

/// Normalize a raw preference string to `system` | `zh-CN` | `en`.
pub fn normalize_ui_locale_preference(raw: &str) -> &'static str {
    match raw.trim() {
        "zh-CN" => "zh-CN",
        "en" => "en",
        "system" => "system",
        other => {
            if other.is_empty() {
                log::warn!("i18n: uiLocale missing/empty; falling back to system");
            } else {
                log::warn!("i18n: unknown uiLocale preference {other:?}; treating as system");
            }
            "system"
        }
    }
}

/// Resolve preference + OS language to a catalog locale (`system`: zh* → zh-CN, else en).
/// When `host_language` is `Some`, use it instead of detecting the OS locale.
pub fn resolve_ui_locale(pref: &str) -> UiLocale {
    resolve_ui_locale_with_host(pref, None)
}

/// Same as [`resolve_ui_locale`] with an optional host language override (tests / injection).
pub fn resolve_ui_locale_with_host(pref: &str, host_language: Option<&str>) -> UiLocale {
    match normalize_ui_locale_preference(pref) {
        "zh-CN" => UiLocale::ZhCn,
        "en" => UiLocale::En,
        _ => match host_language.map(str::trim).filter(|s| !s.is_empty()) {
            Some(tag) => {
                if tag.to_ascii_lowercase().starts_with("zh") {
                    UiLocale::ZhCn
                } else {
                    UiLocale::En
                }
            }
            None => detect_system_ui_locale(),
        },
    }
}

/// English instruction telling the model which UI language to reply in.
pub fn ui_locale_reply_rule(locale: UiLocale) -> &'static str {
    match locale {
        UiLocale::ZhCn => {
            "Reply in Simplified Chinese (zh-CN) to match the user's UI language."
        }
        UiLocale::En => "Reply in English to match the user's UI language.",
    }
}

/// Append the UI-language reply rule to cacheable system prompts.
pub fn push_ui_locale_reply_rule_to_cacheable(cacheable: &mut Vec<String>, ui_locale_pref: &str) {
    let resolved = resolve_ui_locale(ui_locale_pref);
    let rule = ui_locale_reply_rule(resolved);
    cacheable.push(format!("[UI Language]\n{rule}"));
    log::info!(
        "i18n: injected UI language reply rule pref={} resolved={}",
        normalize_ui_locale_preference(ui_locale_pref),
        resolved.as_str()
    );
}

/// Load `UserSettings.ui_locale` and resolve (warns on settings load failure).
pub fn current_ui_locale() -> UiLocale {
    match crate::storage::load_user_settings() {
        Ok(s) => resolve_ui_locale(&s.ui_locale),
        Err(e) => {
            log::warn!("i18n: failed to load user settings for uiLocale: {e:#}; using system");
            resolve_ui_locale("system")
        }
    }
}

/// Translate `key` for `locale`. Missing keys log a warning and return the key (never empty silence).
pub fn t(locale: UiLocale, key: &str) -> String {
    match catalogs::lookup(locale, key) {
        Some(s) => s.to_string(),
        None => {
            log::warn!(
                "i18n: missing key {key:?} for locale={}",
                locale.as_str()
            );
            key.to_string()
        }
    }
}

/// Translate with `{name}` placeholders. `args` are `(name, value)` pairs.
pub fn tf(locale: UiLocale, key: &str, args: &[(&str, &str)]) -> String {
    let mut out = t(locale, key);
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// User-facing "generation stopped" message for the current UI locale.
pub fn generation_stopped_msg() -> String {
    t(current_ui_locale(), "errors.generationStopped")
}

fn detect_system_ui_locale() -> UiLocale {
    match detect_os_language_tag() {
        Some(tag) => {
            let lower = tag.trim().to_ascii_lowercase();
            if lower.starts_with("zh") {
                UiLocale::ZhCn
            } else {
                UiLocale::En
            }
        }
        None => {
            log::warn!("i18n: failed to detect OS locale; defaulting to en");
            UiLocale::En
        }
    }
}

fn detect_os_language_tag() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if let Some(tag) = macos_apple_locale_tag() {
            return Some(tag);
        }
    }
    #[cfg(windows)]
    {
        if let Some(tag) = windows_user_locale_name() {
            return Some(tag);
        }
    }
    unix_env_locale_tag()
}

fn unix_env_locale_tag() -> Option<String> {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(key) {
            let t = v.trim();
            if t.is_empty() || t.eq_ignore_ascii_case("C") || t.eq_ignore_ascii_case("POSIX") {
                continue;
            }
            // `zh_CN.UTF-8` / `zh-Hans_CN` → language prefix before `.` / `@`
            let base = t.split(['.', '@']).next().unwrap_or(t).trim();
            if !base.is_empty() {
                return Some(base.replace('_', "-"));
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn macos_apple_locale_tag() -> Option<String> {
    static CACHED: OnceLock<Option<String>> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            let output = std::process::Command::new("defaults")
                .args(["read", "-g", "AppleLocale"])
                .output()
                .ok()?;
            if !output.status.success() {
                log::warn!("i18n: defaults read AppleLocale failed");
                return None;
            }
            let apple = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if apple.is_empty() {
                return None;
            }
            let base = apple.split('@').next().unwrap_or(&apple).trim();
            if base.is_empty() {
                return None;
            }
            Some(base.replace('_', "-"))
        })
        .clone()
}

#[cfg(windows)]
fn windows_user_locale_name() -> Option<String> {
    static CACHED: OnceLock<Option<String>> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            // https://learn.microsoft.com/windows/win32/api/winnls/nf-winnls-getuserdefaultlocalename
            #[link(name = "kernel32")]
            extern "system" {
                fn GetUserDefaultLocaleName(lp_locale_name: *mut u16, cch_locale_name: i32) -> i32;
            }
            const LOCALE_NAME_MAX_LENGTH: usize = 85;
            let mut buf = [0u16; LOCALE_NAME_MAX_LENGTH];
            let len = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
            if len <= 1 {
                log::warn!("i18n: GetUserDefaultLocaleName failed or empty");
                return None;
            }
            let s = String::from_utf16_lossy(&buf[..(len as usize - 1)]);
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_known_prefs() {
        assert_eq!(normalize_ui_locale_preference("zh-CN"), "zh-CN");
        assert_eq!(normalize_ui_locale_preference("en"), "en");
        assert_eq!(normalize_ui_locale_preference("system"), "system");
        assert_eq!(normalize_ui_locale_preference("de"), "system");
    }

    #[test]
    fn resolve_explicit_locales() {
        assert_eq!(resolve_ui_locale("zh-CN"), UiLocale::ZhCn);
        assert_eq!(resolve_ui_locale("en"), UiLocale::En);
    }

    #[test]
    fn t_returns_zh_and_en_for_agents() {
        assert_eq!(t(UiLocale::ZhCn, "agents.general"), "通用助手");
        assert_eq!(t(UiLocale::En, "agents.general"), "General");
    }

    #[test]
    fn tf_interpolates_named_args() {
        assert_eq!(
            tf(UiLocale::ZhCn, "tools.waitSecs", &[("secs", "3")]),
            "等待 3 秒"
        );
        assert_eq!(
            tf(UiLocale::En, "tools.waitSecs", &[("secs", "3")]),
            "Wait 3s"
        );
    }

    #[test]
    fn missing_key_returns_key_not_empty() {
        let got = t(UiLocale::En, "tools.__missing_key__");
        assert_eq!(got, "tools.__missing_key__");
    }

    #[test]
    fn catalog_parity_tools_and_agents() {
        for key in catalogs::ALL_KEYS {
            assert!(
                catalogs::lookup(UiLocale::ZhCn, key).is_some(),
                "zh-CN missing {key}"
            );
            assert!(
                catalogs::lookup(UiLocale::En, key).is_some(),
                "en missing {key}"
            );
        }
    }

    #[test]
    fn reply_rule_texts() {
        assert_eq!(
            ui_locale_reply_rule(UiLocale::ZhCn),
            "Reply in Simplified Chinese (zh-CN) to match the user's UI language."
        );
        assert_eq!(
            ui_locale_reply_rule(UiLocale::En),
            "Reply in English to match the user's UI language."
        );
    }

    #[test]
    fn push_injects_zh_cn_and_en_rules() {
        let mut zh = Vec::new();
        push_ui_locale_reply_rule_to_cacheable(&mut zh, "zh-CN");
        assert_eq!(zh.len(), 1);
        assert_eq!(
            zh[0],
            "[UI Language]\nReply in Simplified Chinese (zh-CN) to match the user's UI language."
        );

        let mut en = Vec::new();
        push_ui_locale_reply_rule_to_cacheable(&mut en, "en");
        assert_eq!(en.len(), 1);
        assert_eq!(
            en[0],
            "[UI Language]\nReply in English to match the user's UI language."
        );
    }

    #[test]
    fn resolve_with_host_override() {
        assert_eq!(
            resolve_ui_locale_with_host("system", Some("zh-CN")),
            UiLocale::ZhCn
        );
        assert_eq!(
            resolve_ui_locale_with_host("system", Some("en-US")),
            UiLocale::En
        );
    }
}
