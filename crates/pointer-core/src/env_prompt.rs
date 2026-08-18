//! Environment snippets: **calendar date only** in trailing `[Environment]` `system` text;
//! **full date+time** in Computer `[CUR_SCREEN]` inject.
//! Both include brief **OS** and **Time baseline** usage lines for every agent.

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

fn locale_hint() -> String {
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
pub fn build_environment_system_prompt_slice() -> String {
    let os = os_label();
    format!(
        "Environment:\n- OS: {os}\n{}\n- Locale hint: {}\n- Local date: {}\n{}",
        os_usage(os),
        locale_hint(),
        format_local_date_calendar(),
        time_baseline_usage("Local date")
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
    }
}
