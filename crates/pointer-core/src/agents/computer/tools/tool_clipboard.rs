//! System clipboard plain text (PyProjects `clipboard.py` alignment).

use super::args_util::require_non_empty_str;
use anyhow::{anyhow, Result};
use arboard::Clipboard;
use log::{info, warn};
use serde_json::Value;

const CLIPBOARD_MAX_RETURN_CHARS: usize = 100_000;

/// Clipboard tool: `read` / `write` (XML: **`clipboard.read`**, **`clipboard.write`**).
pub struct ClipboardTool;

impl ClipboardTool {
    pub fn new() -> Self {
        Self
    }

    pub fn execute(&self, method: &str, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        match method {
            "read" | "read_clipboard" => self.read_clipboard(args),
            "write" | "write_clipboard" => self.write_clipboard(args),
            _ => Err(anyhow!(
                "Unknown clipboard method: {}. Use read (clipboard:read) or write (clipboard:write).",
                method
            )),
        }
    }

    fn read_clipboard(&self, args: &Value) -> Result<String> {
        let goal = args["goal"].as_str().unwrap_or("").trim();
        let mut cb = Clipboard::new().map_err(|e| anyhow!("Clipboard unavailable: {e}"))?;
        let text = cb.get_text().map_err(|e| anyhow!("read_clipboard failed: {e}"))?;
        let (body, truncated) = truncate_clipboard_reply(&text);
        let note = if truncated {
            " (truncated in reply)"
        } else {
            ""
        };
        if truncated {
            warn!(
                "clipboard read_clipboard: reply truncated to {} bytes (limit {})",
                body.len(),
                CLIPBOARD_MAX_RETURN_CHARS
            );
        }
        info!(
            "clipboard read_clipboard: goal_len={} text_len={} truncated={}",
            goal.chars().count(),
            body.len(),
            truncated
        );
        Ok(format!(
            "Goal: {}. Clipboard text{}:\n\n{}",
            goal, note, body
        ))
    }

    fn write_clipboard(&self, args: &Value) -> Result<String> {
        let goal = args["goal"].as_str().unwrap_or("").trim();
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing 'text' in tool_args for write_clipboard"))?;
        let mut cb = Clipboard::new().map_err(|e| anyhow!("Clipboard unavailable: {e}"))?;
        cb.set_text(text)
            .map_err(|e| anyhow!("write_clipboard failed: {e}"))?;
        info!(
            "clipboard write_clipboard: goal_len={} chars_written={}",
            goal.chars().count(),
            text.len()
        );
        Ok(format!(
            "Goal: {}. Copied {} characters to clipboard. Nothing was pasted — use hotkey (paste) or input if needed.",
            goal,
            text.len()
        ))
    }
}

/// Host verify shortcut — skip Verify LLM when the outcome is unambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardHostVerifyKind {
    /// Non-empty read or successful write — let Verify LLM compare content vs goal.
    Defer,
    /// `clipboard_read` returned an empty body.
    EmptyClipboard,
    /// `clipboard_write` reported zero characters copied.
    ZeroWritten,
    /// Missing tool text, execution error, or unparseable reply.
    ToolError,
}

pub fn clipboard_host_verify_kind(tool_name: &str, tool_text: Option<&str>) -> ClipboardHostVerifyKind {
    let name = tool_name.trim().to_ascii_lowercase();
    let Some(text) = tool_text else {
        return ClipboardHostVerifyKind::ToolError;
    };
    if text.trim().is_empty() {
        return ClipboardHostVerifyKind::ToolError;
    }
    match name.as_str() {
        "clipboard_read" => match extract_clipboard_read_body(text) {
            Some(body) if body.trim().is_empty() => ClipboardHostVerifyKind::EmptyClipboard,
            Some(_) => ClipboardHostVerifyKind::Defer,
            None => ClipboardHostVerifyKind::ToolError,
        },
        "clipboard_write" => {
            if text.contains("Copied 0 characters to clipboard") {
                ClipboardHostVerifyKind::ZeroWritten
            } else if text.contains("Copied ") && text.contains(" characters to clipboard") {
                ClipboardHostVerifyKind::Defer
            } else {
                ClipboardHostVerifyKind::ToolError
            }
        }
        _ => ClipboardHostVerifyKind::Defer,
    }
}

/// Body after the `Clipboard text:` header in a successful `read_clipboard` reply.
pub fn extract_clipboard_read_body(tool_text: &str) -> Option<&str> {
    let needle = ". Clipboard text";
    let suffix = tool_text.get(tool_text.find(needle)? + needle.len()..)?;
    let suffix = suffix
        .strip_prefix(" (truncated in reply)")
        .unwrap_or(suffix);
    suffix
        .strip_prefix(":\n\n")
        .or_else(|| suffix.strip_prefix(':').filter(|r| r.is_empty()))
}

fn truncate_clipboard_reply(text: &str) -> (&str, bool) {
    if text.len() <= CLIPBOARD_MAX_RETURN_CHARS {
        return (text, false);
    }
    let mut end = CLIPBOARD_MAX_RETURN_CHARS;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

impl Default for ClipboardTool {
    fn default() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_read_body_non_empty() {
        let t = "Goal: Read key. Clipboard text:\n\nsk-abc";
        assert_eq!(extract_clipboard_read_body(t), Some("sk-abc"));
    }

    #[test]
    fn extract_read_body_empty() {
        let t = "Goal: Read key. Clipboard text:\n\n";
        assert_eq!(extract_clipboard_read_body(t), Some(""));
    }

    #[test]
    fn host_verify_empty_clipboard_skips_llm() {
        let t = "Goal: Read key. Clipboard text:\n\n";
        assert_eq!(
            clipboard_host_verify_kind("clipboard_read", Some(t)),
            ClipboardHostVerifyKind::EmptyClipboard
        );
    }

    #[test]
    fn host_verify_non_empty_deferrs() {
        let t = "Goal: Read key. Clipboard text:\n\nhello";
        assert_eq!(
            clipboard_host_verify_kind("clipboard_read", Some(t)),
            ClipboardHostVerifyKind::Defer
        );
    }

    #[test]
    fn host_verify_write_zero_chars() {
        let t = "Goal: Copy. Copied 0 characters to clipboard. Nothing was pasted — use hotkey (paste) or input if needed.";
        assert_eq!(
            clipboard_host_verify_kind("clipboard_write", Some(t)),
            ClipboardHostVerifyKind::ZeroWritten
        );
    }

    #[test]
    fn host_verify_empty_after_trimmed_tool_text() {
        let t = "Goal: Read key. Clipboard text:";
        assert_eq!(
            clipboard_host_verify_kind("clipboard_read", Some(t)),
            ClipboardHostVerifyKind::EmptyClipboard
        );
    }

    #[test]
    fn truncate_unicode_boundary() {
        let s = "a".repeat(CLIPBOARD_MAX_RETURN_CHARS + 4);
        let (t, tr) = truncate_clipboard_reply(&s);
        assert!(tr);
        assert!(t.len() <= CLIPBOARD_MAX_RETURN_CHARS);
        assert!(t.is_char_boundary(t.len()));
    }
}
