//! System clipboard plain text (PyProjects `clipboard.py` alignment).

use super::args_util::require_non_empty_str;
use anyhow::{anyhow, Result};
use arboard::Clipboard;
use log::{info, warn};
use serde_json::Value;

const CLIPBOARD_MAX_RETURN_CHARS: usize = 100_000;

/// Clipboard tool: `read` / `write` (XML: **`clipboard:read`**, **`clipboard:write`**).
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
            "Goal: {}. Copied {} characters to clipboard. Nothing was pasted — use hotkey (paste) or composite_action if needed.",
            goal,
            text.len()
        ))
    }
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
    fn truncate_unicode_boundary() {
        let s = "a".repeat(CLIPBOARD_MAX_RETURN_CHARS + 4);
        let (t, tr) = truncate_clipboard_reply(&s);
        assert!(tr);
        assert!(t.len() <= CLIPBOARD_MAX_RETURN_CHARS);
        assert!(t.is_char_boundary(t.len()));
    }
}
