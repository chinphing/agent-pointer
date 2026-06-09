//! Shared outbound message formatting for IM channels that support Markdown.

use serde_json::json;

/// Short title for DingTalk `markdown.title` (required field).
pub fn dingtalk_markdown_title(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("Pointer");
    let stripped = line.trim_start_matches('#').trim();
    let title = if stripped.is_empty() { "Pointer" } else { stripped };
    const MAX: usize = 32;
    if title.chars().count() <= MAX {
        title.to_string()
    } else {
        title.chars().take(MAX).collect()
    }
}

/// Feishu `msg_type: post` content JSON string with a single `md` paragraph.
pub fn feishu_post_md_content(text: &str) -> anyhow::Result<String> {
    let inner = json!({
        "zh_cn": {
            "title": "",
            "content": [[{ "tag": "md", "text": text }]]
        }
    });
    Ok(serde_json::to_string(&inner)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dingtalk_title_strips_heading_markers() {
        assert_eq!(dingtalk_markdown_title("## Hello\nbody"), "Hello");
    }

    #[test]
    fn dingtalk_title_truncates_long_lines() {
        let long = "a".repeat(40);
        assert_eq!(dingtalk_markdown_title(&long).chars().count(), 32);
    }

    #[test]
    fn feishu_post_md_wraps_content() {
        let content = feishu_post_md_content("**bold**").unwrap();
        assert!(content.contains("\"tag\":\"md\""));
        assert!(content.contains("**bold**"));
    }
}
