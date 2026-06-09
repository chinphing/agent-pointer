//! IM session reset: manual commands and idle-based rotation.

pub const MANUAL_RESET_ACK: &str = "已开始新对话。";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualResetAction {
    /// User sent only a reset command; reply with ack and skip chat.
    ResetOnly,
    /// Reset then continue with the remaining user text.
    ResetWithMessage(String),
}

/// Detect `/new`, `/reset`, `新对话`, `重新开始` in inbound text.
pub fn detect_manual_reset(text: &str) -> Option<ManualResetAction> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    for cmd in ["/new", "/reset"] {
        if lower == cmd {
            return Some(ManualResetAction::ResetOnly);
        }
        let prefix = format!("{cmd} ");
        if lower.starts_with(&prefix) {
            let rest = trimmed[prefix.len()..].trim().to_string();
            return Some(if rest.is_empty() {
                ManualResetAction::ResetOnly
            } else {
                ManualResetAction::ResetWithMessage(rest)
            });
        }
    }

    let cn = trimmed.trim_end_matches(['。', '！', '!', '？', '?', '.', ' ']);
    match cn {
        "新对话" | "重新开始" => Some(ManualResetAction::ResetOnly),
        _ => None,
    }
}

pub fn should_idle_reset(last_interaction_at_ms: i64, idle_minutes: u32, now_ms: i64) -> bool {
    if idle_minutes == 0 || last_interaction_at_ms <= 0 {
        return false;
    }
    let threshold_ms = i64::from(idle_minutes) * 60 * 1000;
    now_ms.saturating_sub(last_interaction_at_ms) > threshold_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_slash_commands() {
        assert_eq!(
            detect_manual_reset("/new"),
            Some(ManualResetAction::ResetOnly)
        );
        assert_eq!(
            detect_manual_reset("/reset"),
            Some(ManualResetAction::ResetOnly)
        );
        assert_eq!(
            detect_manual_reset("/NEW"),
            Some(ManualResetAction::ResetOnly)
        );
        assert_eq!(
            detect_manual_reset("/new 你好"),
            Some(ManualResetAction::ResetWithMessage("你好".into()))
        );
        assert_eq!(detect_manual_reset("hello"), None);
    }

    #[test]
    fn detects_chinese_phrases() {
        assert_eq!(
            detect_manual_reset("新对话"),
            Some(ManualResetAction::ResetOnly)
        );
        assert_eq!(
            detect_manual_reset("重新开始！"),
            Some(ManualResetAction::ResetOnly)
        );
        assert_eq!(detect_manual_reset("开始新对话"), None);
    }

    #[test]
    fn idle_reset_threshold() {
        let now = 1_700_000_000_000_i64;
        assert!(!should_idle_reset(0, 60, now));
        assert!(!should_idle_reset(now - 30 * 60 * 1000, 60, now));
        assert!(should_idle_reset(now - 61 * 60 * 1000, 60, now));
        assert!(!should_idle_reset(now - 61 * 60 * 1000, 0, now));
    }
}
