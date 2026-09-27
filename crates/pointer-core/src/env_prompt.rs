//! Host environment slice for system prompts (OS, locale, calendar date).
//! Reply-language guidance follows the user's UI locale preference.

use chrono::Local;
use std::env;

/// Full local wall-clock string (date + time + zone label). Used in Computer screen inject.
pub fn format_local_wall_clock_full() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S %Z").to_string()
}

/// Calendar local date `YYYY-MM-DD` for the trailing `[Environment]` system slice.
pub fn format_local_date_calendar() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn os_label() -> &'static str {
    match env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    }
}

fn locale_hint_from_os() -> String {
    env::var("LANG")
        .ok()
        .and_then(|lang| {
            let lang = lang.split('.').next().unwrap_or(&lang);
            if lang.starts_with("zh") {
                Some("Chinese".into())
            } else if lang.starts_with("en") {
                Some("English".into())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".into())
}

/// Resolve UI preference (`system` | `zh-CN` | `en`) to a reply-language label for the model.
pub fn resolve_reply_language_label(ui_locale: &str) -> &'static str {
    match ui_locale.trim() {
        "zh-CN" => "Chinese (Simplified)",
        "en" => "English",
        _ => {
            // system / unknown → follow OS locale hint
            if locale_hint_from_os().starts_with("Chinese") {
                "Chinese (Simplified)"
            } else {
                "English"
            }
        }
    }
}

fn reply_language_rule(ui_locale: &str) -> String {
    let lang = resolve_reply_language_label(ui_locale);
    format!(
        "- Reply language: Reply to the user in **{lang}** unless they clearly write \
in another language or ask otherwise. Keep tool arguments, code identifiers, and \
file paths unchanged."
    )
}

fn os_usage(os: &str) -> String {
    format!(
        "- OS usage: **{os}** above is the user's host platform for this session. Platform-typical \
paths, shell syntax, keyboard shortcuts, and native tooling apply to {os} unless the user \
targets another OS."
    )
}

fn time_baseline_usage(reference_label: &str) -> String {
    format!(
        "- Time baseline: Treat **{reference_label}** above as this session's authoritative clock \
unless the user states another date or time. When timing is unspecified, prefer information \
current relative to {reference_label}—not stale training defaults or guessed years."
    )
}

/// OS + locale + **calendar date only** — last slice of `system_prompts` for `stream_chat`.
///
/// `ui_locale` is the persisted preference (`system` | `zh-CN` | `en`).
pub fn build_environment_system_prompt_slice() -> String {
    build_environment_system_prompt_slice_for("system")
}

/// Same as [`build_environment_system_prompt_slice`] with an explicit UI locale preference.
pub fn build_environment_system_prompt_slice_for(ui_locale: &str) -> String {
    let os = os_label();
    format!(
        "Environment:\n- OS: {os}\n{}\n- Locale hint: {}\n- Local date: {}\n{}\n{}",
        os_usage(os),
        locale_hint_from_os(),
        format_local_date_calendar(),
        time_baseline_usage("Local date"),
        reply_language_rule(ui_locale)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_slice_includes_time_baseline_for_local_date() {
        let slice = build_environment_system_prompt_slice();
        assert!(slice.contains("Local date:"));
        assert!(slice.contains("OS usage:"));
        assert!(slice.contains("host platform"));
        assert!(slice.contains("Time baseline:"));
        assert!(slice.contains("**Local date**"));
        assert!(slice.contains("authoritative clock"));
        assert!(slice.contains("Reply language:"));
    }

    #[test]
    fn reply_language_follows_explicit_ui_locale() {
        let zh = build_environment_system_prompt_slice_for("zh-CN");
        assert!(zh.contains("Chinese (Simplified)"));
        let en = build_environment_system_prompt_slice_for("en");
        assert!(en.contains("**English**"));
    }
}
