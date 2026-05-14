//! Environment snippets: **calendar date only** in trailing `[Environment]` `system` text;
//! **full date+time** in Computer `[CUR_SCREEN]` inject and Supervisor `chat_once` templates.

use chrono::Local;
use std::env;

/// Full local wall-clock string (date + time + zone label). Used in Computer screen inject and Supervisor `chat_once` paths.
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

/// OS + locale + **calendar date only** — last slice of `system_prompts` for `stream_chat`.
pub fn build_environment_system_prompt_slice() -> String {
    format!(
        "Environment:\n- OS: {}\n- Locale hint: {}\n- Local date: {}",
        os_label(),
        locale_hint(),
        format_local_date_calendar()
    )
}

/// OS + locale + full local time — embedded in Supervisor `chat_once` templates.
pub fn build_environment_context_full() -> String {
    format!(
        "Environment:\n- OS: {}\n- Locale hint: {}\n- Local time: {}",
        os_label(),
        locale_hint(),
        format_local_wall_clock_full()
    )
}
