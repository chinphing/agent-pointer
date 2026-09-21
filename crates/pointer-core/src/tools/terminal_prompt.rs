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
    // OpenSSH 8.8+ host-key confirm: (yes/no/[fingerprint]) — no literal "(yes/no)"
    "(yes/no/",
    "[yes/no/",
    "y/n?",
    "yes/no?",
    "continue connecting",
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

    let trimmed_end = lower.trim_end();
    let last_line = last_meaningful_line(&tail);
    let needs_input_likely = input_class == InputClass::Secret
        || NORMAL_PROMPT_MARKERS.iter().any(|m| lower.contains(m))
        || trimmed_end.ends_with('?')
        || looks_like_colon_prompt(&last_line);

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

const INPUT_CONTEXT_MAX_LINES: usize = 10;
const INPUT_CONTEXT_MAX_CHARS: usize = 1200;

/// Recent terminal output shown in the input modal so the user can see the prompt.
pub fn input_context_snippet(combined_output: &str) -> Option<String> {
    let cleaned = strip_ansi_escapes(combined_output).trim().to_string();
    if cleaned.is_empty() {
        return None;
    }
    let mut tail = tail_lines(&cleaned, INPUT_CONTEXT_MAX_LINES);
    if tail.len() > INPUT_CONTEXT_MAX_CHARS {
        let skip = tail.len().saturating_sub(INPUT_CONTEXT_MAX_CHARS);
        tail = tail.chars().skip(skip).collect();
    }
    Some(tail)
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

/// `Username:` / `Enter path:` — not section banners like `=== 启动 attachments:`.
/// Also not hostname / file prefixes (`cipz.pfms:`) from `printf "host: "; curl`.
fn looks_like_colon_prompt(last_line: &str) -> bool {
    let t = last_line.trim();
    if !t.ends_with(':') {
        return false;
    }
    let body = t[..t.len() - 1].trim_end();
    if body.is_empty() {
        return false;
    }
    if body.starts_with('=')
        || body.starts_with('-')
        || body.starts_with('#')
        || body.starts_with('*')
    {
        return false;
    }
    // Echoed labels and JSON keys are longer than a real stdin prompt.
    if t.len() > 48 {
        return false;
    }
    // Dotted tokens are hosts or filenames, not `Username:` / `Enter path:`.
    if body.contains('.') && !body.contains(char::is_whitespace) {
        return false;
    }
    true
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

/// Short idle before treating trailing output as an interactive prompt.
/// Separate from the command `timeoutMs` so yes/no → password can re-prompt quickly.
pub const PROMPT_DETECT_IDLE_MS: u64 = 800;

/// After the user already answered an interactive prompt (e.g. SSH host-key yes/no),
/// OpenSSH may wait at a password prompt that does not clearly echo `password:` on
/// the PTY. Use only after prior input — never on first connect (key auth / hang).
pub fn post_interactive_ssh_secret_prompt(
    command: &str,
    combined_output: &str,
    idle_ms: u64,
    user_already_input: bool,
) -> Option<PromptState> {
    if !user_already_input || idle_ms < PROMPT_DETECT_IDLE_MS {
        return None;
    }
    if !super::terminal_askpass::command_wants_ssh_askpass(command) {
        return None;
    }
    let state = detect_prompt_state(combined_output, idle_ms);
    if state.needs_input_likely {
        return None;
    }
    Some(PromptState {
        needs_input_likely: true,
        input_hint: Some("SSH password".to_string()),
        input_class: InputClass::Secret,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_context_snippet_takes_tail() {
        let text = (0..20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let snippet = input_context_snippet(&text).unwrap();
        assert!(snippet.contains("line 19"));
        assert!(!snippet.contains("line 0"));
    }

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
    fn ignores_ssh_connecting_without_password_prompt() {
        let state = detect_prompt_state("Connecting to host...\n", 5000);
        assert!(!state.needs_input_likely);
    }

    #[test]
    fn ignores_echoed_section_header_ending_with_colon() {
        let text = r#"{ "status": "needs_flexible_or_supplement" }
=== 启动fast attachments:"#;
        let state = detect_prompt_state(text, 800);
        assert!(!state.needs_input_likely);
    }

    #[test]
    fn still_detects_short_colon_prompts() {
        let state = detect_prompt_state("Username: ", 800);
        assert!(state.needs_input_likely);
        assert_eq!(state.input_class, InputClass::Normal);
    }

    #[test]
    fn ignores_hostname_colon_prefix_while_curl_hangs() {
        let state = detect_prompt_state("cipz.pfms: ", 800);
        assert!(!state.needs_input_likely);
        let state = detect_prompt_state("cipz.pfms: 302 0\n", 800);
        assert!(!state.needs_input_likely);
    }

    #[test]
    fn detects_ssh_host_key_yes_no() {
        let text = "The authenticity of host '1.2.3.4' can't be established.\n\
                    ED25519 key fingerprint is SHA256:abc.\n\
                    Are you sure you want to continue connecting (yes/no/[fingerprint])? ";
        let state = detect_prompt_state(text, 800);
        assert!(state.needs_input_likely);
        assert_eq!(state.input_class, InputClass::Normal);
    }

    #[test]
    fn detects_password_after_host_key_trust() {
        let text = "Warning: Permanently added '1.2.3.4' (ED25519) to the list of known hosts.\n\
                    root@1.2.3.4's password: ";
        let state = detect_prompt_state(text, 800);
        assert!(state.needs_input_likely);
        assert_eq!(state.input_class, InputClass::Secret);
    }

    #[test]
    fn post_interactive_ssh_secret_only_after_prior_input() {
        let out = "Warning: Permanently added '1.2.3.4' (ED25519) to the list of known hosts.\n";
        assert!(post_interactive_ssh_secret_prompt("ssh root@1.2.3.4", out, 800, false).is_none());
        let p = post_interactive_ssh_secret_prompt("ssh root@1.2.3.4", out, 800, true).unwrap();
        assert_eq!(p.input_class, InputClass::Secret);
        assert!(post_interactive_ssh_secret_prompt(
            "ssh root@1.2.3.4",
            "root@host's password: ",
            800,
            true
        )
        .is_none());
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
}
