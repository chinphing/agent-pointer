//! Minimal UI-string i18n layer for Pointer's Rust backend.
//!
//! This module resolves the effective UI locale (from `UserSettings.ui_locale`, falling
//! back to the OS locale) and renders final, ready-to-display strings for the small set of
//! user-visible surfaces owned by the Rust host: tool display labels/summaries, agent
//! composer labels, `UiToast` messages, and a handful of common user-facing `Err` strings.
//!
//! The frontend renders whatever string is returned here as-is — it does not re-translate.
//!
//! LLM system prompts are intentionally out of scope here (tracked separately).

mod catalogs;

use crate::models::UserSettings;

/// Effective UI locale used to render backend-originated user-visible strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiLocale {
    ZhCn,
    En,
}

impl UiLocale {
    pub fn as_str(&self) -> &'static str {
        match self {
            UiLocale::ZhCn => "zh-CN",
            UiLocale::En => "en",
        }
    }
}

/// Resolve the effective [`UiLocale`] from user settings.
///
/// `ui_locale` is one of `"system"` (default), `"zh-CN"`, or `"en"`. When set to `"system"`
/// (or any other unrecognized value), the OS/environment locale is consulted.
pub fn resolve_ui_locale(user: &UserSettings) -> UiLocale {
    let pref = user.ui_locale.trim();
    let locale = match pref {
        "zh-CN" | "zh-Hans" | "zh_CN" => UiLocale::ZhCn,
        "en" | "en-US" | "en_US" => UiLocale::En,
        _ => resolve_system_ui_locale(),
    };
    log::debug!(
        "i18n: resolved ui_locale pref={:?} -> {}",
        pref,
        locale.as_str()
    );
    locale
}

/// Best-effort OS/environment locale detection without pulling in a new dependency.
///
/// Checks `LC_ALL`, `LC_MESSAGES`, then `LANG` for a `zh*` prefix; anything else (including
/// unset/unparseable values) defaults to English.
fn resolve_system_ui_locale() -> UiLocale {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            let lower = value.trim().to_ascii_lowercase();
            if lower.starts_with("zh") {
                return UiLocale::ZhCn;
            }
            if !lower.is_empty() && lower != "c" && lower != "posix" {
                // A concrete non-Chinese locale was found; stop looking further.
                return UiLocale::En;
            }
        }
    }
    UiLocale::En
}

/// Convenience for call sites that don't already have `UserSettings` in scope: loads the
/// on-disk user settings (falling back to defaults on error) and resolves the locale.
pub fn current_ui_locale() -> UiLocale {
    let user = crate::storage::load_user_settings().unwrap_or_default();
    resolve_ui_locale(&user)
}

/// Look up the final display string for `key` in `locale`.
///
/// Falls back to the English catalog, then to the key itself, logging a warning when a key
/// is missing from both catalogs (so gaps are visible without crashing the UI).
pub fn t(key: &str, locale: UiLocale) -> &'static str {
    let found = match locale {
        UiLocale::ZhCn => catalogs::lookup_zh_cn(key),
        UiLocale::En => catalogs::lookup_en(key),
    };
    if let Some(s) = found {
        return s;
    }
    if let Some(s) = catalogs::lookup_en(key) {
        log::warn!("i18n: missing key {key:?} for locale {locale:?}, falling back to en");
        return s;
    }
    log::warn!("i18n: unknown i18n key {key:?}, falling back to key itself");
    Box::leak(key.to_string().into_boxed_str())
}

/// Like [`t`], but substitutes `{name}` placeholders from `args` and returns an owned string.
pub fn tf(key: &str, locale: UiLocale, args: &[(&str, &str)]) -> String {
    let template = t(key, locale);
    let mut out = template.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_with_locale(locale: &str) -> UserSettings {
        let mut s = UserSettings::default();
        s.ui_locale = locale.to_string();
        s
    }

    #[test]
    fn resolve_ui_locale_respects_explicit_preference() {
        assert_eq!(resolve_ui_locale(&settings_with_locale("zh-CN")), UiLocale::ZhCn);
        assert_eq!(resolve_ui_locale(&settings_with_locale("en")), UiLocale::En);
    }

    #[test]
    fn resolve_ui_locale_system_checks_env() {
        let prev_lc_all = std::env::var("LC_ALL").ok();
        let prev_lang = std::env::var("LANG").ok();
        std::env::set_var("LC_ALL", "zh_CN.UTF-8");
        assert_eq!(resolve_ui_locale(&settings_with_locale("system")), UiLocale::ZhCn);
        std::env::set_var("LC_ALL", "en_US.UTF-8");
        assert_eq!(resolve_ui_locale(&settings_with_locale("system")), UiLocale::En);
        match prev_lc_all {
            Some(v) => std::env::set_var("LC_ALL", v),
            None => std::env::remove_var("LC_ALL"),
        }
        match prev_lang {
            Some(v) => std::env::set_var("LANG", v),
            None => std::env::remove_var("LANG"),
        }
    }

    #[test]
    fn default_settings_use_system_locale() {
        let s = UserSettings::default();
        assert_eq!(s.ui_locale, "system");
    }

    #[test]
    fn t_returns_locale_specific_strings() {
        assert_eq!(t("tool.file.read", UiLocale::ZhCn), "读取文件");
        assert_eq!(t("tool.file.read", UiLocale::En), "Read file");
    }

    #[test]
    fn t_falls_back_to_en_then_key_marker_for_unknown_keys() {
        assert_eq!(t("no.such.key", UiLocale::ZhCn), "no.such.key");
        assert_eq!(t("no.such.key", UiLocale::En), "no.such.key");
    }

    #[test]
    fn tf_substitutes_named_placeholders() {
        let out = tf(
            "tool.wait.label",
            UiLocale::ZhCn,
            &[("secs", "5")],
        );
        assert_eq!(out, "等待 5 秒");
        let out_en = tf("tool.wait.label", UiLocale::En, &[("secs", "5")]);
        assert_eq!(out_en, "Wait 5s");
    }

    #[test]
    fn tf_handles_multiple_placeholders() {
        let out = tf(
            "toast.compress.success_sub",
            UiLocale::ZhCn,
            &[("name", "编码"), ("dropped", "12")],
        );
        assert_eq!(out, "编码 子任务：已压缩较早 12 条记录为摘要");
    }
}
