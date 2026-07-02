//! WeCom group @-mention detection and text cleanup.

pub struct WecomMentionResult {
    pub mentioned_bot: bool,
    pub text: String,
}

/// Apply group @ rules for WeCom inbound messages.
///
/// - DM: always treated as addressed to the bot.
/// - Bot WSS group: platform only delivers @ messages; still strip `@name` prefix from text.
/// - Agent HTTP webhook group: detect leading `@` in text; media-only group messages are
///   treated as mentioned (WeCom bot group flow requires @).
pub fn apply_wecom_mention(
    text: &str,
    is_group: bool,
    ws_mode: bool,
    has_attachments: bool,
) -> WecomMentionResult {
    if !is_group {
        return WecomMentionResult {
            mentioned_bot: true,
            text: text.to_string(),
        };
    }

    if ws_mode {
        return WecomMentionResult {
            mentioned_bot: true,
            text: strip_leading_at_mention(text),
        };
    }

    let trimmed = text.trim();
    if trimmed.starts_with('@') {
        return WecomMentionResult {
            mentioned_bot: true,
            text: strip_leading_at_mention(text),
        };
    }

    if trimmed.is_empty() && has_attachments {
        return WecomMentionResult {
            mentioned_bot: true,
            text: text.to_string(),
        };
    }

    WecomMentionResult {
        mentioned_bot: false,
        text: text.to_string(),
    }
}

/// Strip a leading `@botname` token from group message text.
pub fn strip_leading_at_mention(text: &str) -> String {
    let trimmed = text.trim_start();
    if !trimmed.starts_with('@') {
        return text.trim().to_string();
    }
    let after_at = trimmed.get(1..).unwrap_or("");
    if let Some(idx) = after_at.find([' ', '\n', '\t', '\r']) {
        after_at[idx..].trim_start().to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dm_always_mentioned() {
        let r = apply_wecom_mention("hello", false, true, false);
        assert!(r.mentioned_bot);
        assert_eq!(r.text, "hello");
    }

    #[test]
    fn ws_group_strips_at_prefix() {
        let r = apply_wecom_mention("@RobotA hello robot", true, true, false);
        assert!(r.mentioned_bot);
        assert_eq!(r.text, "hello robot");
    }

    #[test]
    fn webhook_group_requires_at_in_text() {
        let r = apply_wecom_mention("hello everyone", true, false, false);
        assert!(!r.mentioned_bot);
        assert_eq!(r.text, "hello everyone");
    }

    #[test]
    fn webhook_group_detects_at() {
        let r = apply_wecom_mention("@机器人 这是今日的测试情况", true, false, false);
        assert!(r.mentioned_bot);
        assert_eq!(r.text, "这是今日的测试情况");
    }

    #[test]
    fn webhook_group_media_only_counts_as_mentioned() {
        let r = apply_wecom_mention("", true, false, true);
        assert!(r.mentioned_bot);
    }
}
