use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InputClass {
    Normal,
    Secret,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptState {
    pub needs_input_likely: bool,
    pub input_hint: Option<String>,
    pub input_class: InputClass,
}

const SECRET_PATTERNS: &[&str] = &[
    "password:",
    "password for",
    "'s password:",
    "passphrase:",
    "enter pin",
    "enter otp",
    "one-time password",
    "verification code",
    "mfa code",
    "sudo password",
    "token:",
    "secret:",
];

const NORMAL_PROMPT_MARKERS: &[&str] = &[
    "(y/n)",
    "[y/n]",
    "(yes/no)",
    "[yes/no]",
    "y/n?",
    "yes/no?",
    "continue?",
    "proceed?",
    "enter choice",
    "select an option",
    "press enter",
    "input:",
];

/// Heuristic: trailing output looks like an interactive prompt.
pub fn detect_prompt_state(combined_output: &str, _idle_ms: u64) -> PromptState {
    let cleaned = strip_ansi_escapes(combined_output);
    let tail = tail_lines(&cleaned, 6);
    if tail.is_empty() {
        return PromptState {
            needs_input_likely: false,
            input_hint: None,
            input_class: InputClass::Normal,
        };
    }

    if looks_like_progress_or_log(&tail) {
        return PromptState {
            needs_input_likely: false,
            input_hint: None,
            input_class: InputClass::Normal,
        };
    }

    let lower = tail.to_ascii_lowercase();
    let input_class = if SECRET_PATTERNS.iter().any(|p| lower.contains(p)) {
        InputClass::Secret
    } else {
        InputClass::Normal
    };

    let needs_input_likely = input_class == InputClass::Secret
        || NORMAL_PROMPT_MARKERS.iter().any(|m| lower.contains(m))
        || lower.ends_with('?')
        || lower.ends_with(": ")
        || lower.ends_with(':');

    let input_hint = if needs_input_likely {
        Some(last_meaningful_line(&tail))
    } else {
        None
    };

    PromptState {
        needs_input_likely,
        input_hint,
        input_class,
    }
}

pub fn agent_retry_forbidden(class: InputClass) -> bool {
    class == InputClass::Secret
}

/// Redact lines that echo a secret the user typed via the input modal.
pub fn scrub_secret_echo(text: &str, secret: &str) -> String {
    let secret = secret.trim();
    if secret.is_empty() {
        return text.to_string();
    }
    text.lines()
        .map(|line| {
            if line.contains(secret) {
                "[redacted]".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn tail_lines(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

fn last_meaningful_line(text: &str) -> String {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .chars()
        .take(240)
        .collect()
}

fn looks_like_progress_or_log(tail: &str) -> bool {
    let last = tail
        .lines()
        .rev()
        .find_map(|l| {
            let t = l.trim();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        })
        .unwrap_or("");
    if last.is_empty() {
        return true;
    }
    if last.contains('\u{2588}')
        || last.contains('\u{2592}')
        || (last.starts_with('[') && last.contains('%') && last.ends_with(']'))
    {
        return true;
    }
    let bytes = last.as_bytes();
    if bytes.len() >= 19
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes.get(10) == Some(&b' ')
        && bytes.get(13) == Some(&b':')
    {
        return true;
    }
    false
}

/// Remove ANSI/OSC sequences so PTY output still matches prompt heuristics.
pub fn strip_ansi_escapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.next_if_eq(&'[').is_some() {
                for ch in chars.by_ref() {
                    if ch.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
            if chars.next_if_eq(&']').is_some() {
                for ch in chars.by_ref() {
                    if ch == '\u{7}' {
                        break;
                    }
                }
                continue;
            }
            continue;
        }
        if c == '\r' {
            continue;
        }
        out.push(c);
    }
    out
}

/// SSH on a PTY often waits at a password prompt without printing `password:` clearly.
pub fn proactive_pty_secret_prompt(command: &str, combined_output: &str, idle_ms: u64) -> Option<PromptState> {
    if idle_ms < 1_000 {
        return None;
    }
    if !super::terminal_askpass::command_wants_ssh_askpass(command) {
        return None;
    }
    let cleaned = strip_ansi_escapes(combined_output);
    let state = detect_prompt_state(&cleaned, idle_ms);
    if state.needs_input_likely {
        return None;
    }
    Some(PromptState {
        needs_input_likely: true,
        input_hint: Some("SSH password".to_string()),
        input_class: InputClass::Secret,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_yes_no_prompt() {
        let state = detect_prompt_state("Install package? (y/n) ", 5000);
        assert!(state.needs_input_likely);
        assert_eq!(state.input_class, InputClass::Normal);
        assert!(!agent_retry_forbidden(state.input_class));
    }

    #[test]
    fn detects_secret_password_prompt() {
        let state = detect_prompt_state("Username: admin\nPassword: ", 5000);
        assert!(state.needs_input_likely);
        assert_eq!(state.input_class, InputClass::Secret);
        assert!(agent_retry_forbidden(state.input_class));
    }

    #[test]
    fn ignores_progress_bar_tail() {
        let state = detect_prompt_state("Downloading\n[=====>    ] 55%\n", 5000);
        assert!(!state.needs_input_likely);
    }

    #[test]
    fn scrubs_echoed_secret() {
        let out = scrub_secret_echo("Password: \nmy-secret\nDone", "my-secret");
        assert!(out.contains("[redacted]"));
        assert!(!out.contains("my-secret"));
    }

    #[test]
    fn strip_ansi_before_password_detect() {
        let raw = "\u{1b}[?2004hroot@host's password: \u{1b}[?2004l";
        let state = detect_prompt_state(&raw, 5000);
        assert!(state.needs_input_likely);
        assert_eq!(state.input_class, InputClass::Secret);
    }

    #[test]
    fn proactive_ssh_prompt_on_idle() {
        let p = proactive_pty_secret_prompt("ssh root@1.2.3.4", "Connecting...\n", 5000);
        assert!(p.is_some());
        assert_eq!(p.unwrap().input_class, InputClass::Secret);
    }
}
