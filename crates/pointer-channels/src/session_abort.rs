//! Fast-abort commands for IM channels (aligned with OpenClaw `/stop` fast path).

pub const ABORT_ACK: &str = "已停止当前任务。";

/// Detect `/stop` and common abort phrases before normal dispatch (fast-abort path).
pub fn is_abort_command(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();
    for cmd in ["/stop", "/cancel", "/abort"] {
        if lower == cmd {
            return true;
        }
        let prefix = format!("{cmd} ");
        if lower.starts_with(&prefix) {
            return true;
        }
    }

    let cn = trimmed.trim_end_matches(['。', '！', '!', '？', '?', '.', ' ']);
    matches!(
        cn,
        "停止" | "停下来" | "暂停" | "取消任务" | "停止任务" | "中断"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_slash_stop() {
        assert!(is_abort_command("/stop"));
        assert!(is_abort_command("/STOP"));
        assert!(is_abort_command("/cancel"));
        assert!(is_abort_command("/abort now"));
    }

    #[test]
    fn detects_chinese_abort() {
        assert!(is_abort_command("停止"));
        assert!(is_abort_command("停下来！"));
        assert!(is_abort_command("暂停"));
        assert!(!is_abort_command("不要停止"));
    }
}
