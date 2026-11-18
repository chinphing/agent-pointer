//! System clipboard plain text (PyProjects `clipboard.py` alignment).

use super::args_util::require_non_empty_str;
use crate::agents::computer::control::ComputerControl;
use anyhow::{anyhow, Result};
use log::{info, warn};
use serde_json::Value;

const CLIPBOARD_MAX_RETURN_CHARS: usize = 100_000;

/// Clipboard tool: `read` / `write` (XML: **`clipboard.read`**, **`clipboard.write`**).
pub struct ClipboardTool;

/// The reply `clipboard_write` sends back to the model.
///
/// Counts **characters**, not bytes. `str::len` is UTF-8 bytes, so a
/// two-character CJK goal reported "Copied 6 characters" — the model compared
/// that against its own two-character input, concluded the tool had written
/// something else, and failed an operation that had in fact succeeded. Observed
/// in a real session: `clipboard_write("深圳")` → "Copied 6 characters" →
/// verify fail → retry.
fn write_reply(text: &str) -> String {
    format!(
        "Copied {} characters to clipboard. Nothing was pasted — use hotkey (paste) or input if needed.",
        text.chars().count()
    )
}

/// 容器会话的剪贴板是容器内的 X 选择区，和宿主 Mac 的粘贴板是两套东西。
/// 走到宿主上读，读出来的是用户在 Mac 上复制的内容，写也只是写到 Mac。
fn container_control(args: &Value) -> Option<std::sync::Arc<dyn ComputerControl>> {
    let id = args.get("_conversation_id")?.as_str()?;
    let env = crate::env::session_env::env_for_conversation(id)?;
    if env.kind() != crate::env::EnvKind::Container {
        return None;
    }
    Some(std::sync::Arc::new(
        crate::agents::computer::control::DockerControl::new(env),
    ))
}

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
        let text = match container_control(args) {
            Some(control) => control.get_clipboard_text()?.unwrap_or_default(),
            None => clipboard_read_text()?,
        };
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
        Ok(format!("Clipboard text{}:\n\n{}", note, body))
    }

    fn write_clipboard(&self, args: &Value) -> Result<String> {
        let goal = args["goal"].as_str().unwrap_or("").trim();
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing 'text' in tool_args for write_clipboard"))?;
        match container_control(args) {
            Some(control) => control.set_clipboard_text(text)?,
            None => clipboard_write_text(text)?,
        }
        info!(
            "clipboard write_clipboard: goal_len={} chars_written={}",
            goal.chars().count(),
            text.chars().count()
        );
        Ok(write_reply(text))
    }
}

/// Read the system clipboard as plain text.
///
/// Goes through [`crate::platform::clipboard`], which owns the main-thread dispatch.
fn clipboard_read_text() -> Result<String> {
    crate::platform::clipboard::read_text()
}

/// Write plain text to the system clipboard.
fn clipboard_write_text(text: &str) -> Result<()> {
    crate::platform::clipboard::write_text(text)
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

pub fn clipboard_host_verify_kind(
    tool_name: &str,
    tool_text: Option<&str>,
) -> ClipboardHostVerifyKind {
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
    // 只认 "Clipboard text"，不假设前面有没有别的句子 —— 回复不再回显 `action`，
    // 但旧转录里可能还带着前缀。
    let needle = "Clipboard text";
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
    use serde_json::json;

    /// 容器会话的剪贴板读写必须落在容器里，不能读写 Mac 的粘贴板。
    #[test]
    #[ignore = "需要本机 docker"]
    fn container_clipboard_round_trips_inside_the_container() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::env::provider::ProviderConfig {
            tier: Some(crate::env::provider::Tier::Container),
            backend: Some(crate::env::provider::Backend::Docker),
            sandbox_url: None,
            volume_host_path: None,
            template: Some("pointer-sandbox-container:latest".to_string()),
            admins: Vec::new(),
            accounts: Default::default(),
            platform: true,
        };
        let _ = crate::env::session_env::resolve_session_env(
            &cfg,
            "it-clip-tool",
            "tester",
            Some(dir.path()),
        )
        .expect("resolve")
        .expect("容器环境");

        let tool = ClipboardTool::new();
        let text = "容器剪贴板 0700";
        tool.execute(
            "write",
            &json!({"goal": "写入", "text": text, "_conversation_id": "it-clip-tool"}),
        )
        .expect("写容器剪贴板");

        let reply = tool
            .execute("read", &json!({"goal": "读回", "_conversation_id": "it-clip-tool"}))
            .expect("读容器剪贴板");
        assert!(reply.contains(text), "读回来的不是刚写的内容：{reply}");

        let _ = std::process::Command::new("docker")
            .args(["rm", "-f", "pointer-sbx-it-clip-tool"])
            .output();
    }

    #[test]
    fn extract_read_body_non_empty() {
        let t = "Clipboard text:\n\nsk-abc";
        assert_eq!(extract_clipboard_read_body(t), Some("sk-abc"));
    }

    #[test]
    fn extract_read_body_empty() {
        let t = "Clipboard text:\n\n";
        assert_eq!(extract_clipboard_read_body(t), Some(""));
    }

    #[test]
    fn host_verify_empty_clipboard_skips_llm() {
        let t = "Clipboard text:\n\n";
        assert_eq!(
            clipboard_host_verify_kind("clipboard_read", Some(t)),
            ClipboardHostVerifyKind::EmptyClipboard
        );
    }

    #[test]
    fn host_verify_non_empty_deferrs() {
        let t = "Clipboard text:\n\nhello";
        assert_eq!(
            clipboard_host_verify_kind("clipboard_read", Some(t)),
            ClipboardHostVerifyKind::Defer
        );
    }

    #[test]
    fn write_reply_counts_characters_not_bytes() {
        // 「深圳」= 2 chars / 6 bytes. Reporting 6 made the model think the
        // tool had written something other than what it asked for.
        let r = write_reply("深圳");
        assert!(r.contains("Copied 2 characters"), "{r}");
        assert!(!r.contains("Copied 6 characters"), "{r}");
    }

    #[test]
    fn write_reply_ascii_unchanged() {
        let r = write_reply("hello");
        assert!(r.contains("Copied 5 characters"), "{r}");
    }

    #[test]
    fn host_verify_write_zero_chars() {
        let t = "Copied 0 characters to clipboard. Nothing was pasted — use hotkey (paste) or input if needed.";
        assert_eq!(
            clipboard_host_verify_kind("clipboard_write", Some(t)),
            ClipboardHostVerifyKind::ZeroWritten
        );
    }

    #[test]
    fn host_verify_empty_after_trimmed_tool_text() {
        let t = "Clipboard text:";
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
