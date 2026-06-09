//! Parse outbound media markers from agent replies (OpenClaw `MEDIA:` convention).

use super::path_hint::MEDIA_URI_SCHEME;
use regex::Regex;
use std::sync::OnceLock;

const MEDIA_PREFIX: &str = "MEDIA:";

fn inline_media_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)MEDIA:\s*`?([^\s`\n]+)`?").expect("inline media re")
    })
}

/// Split agent reply into user-visible text and local media path references.
pub fn split_reply_media(reply: &str) -> (String, Vec<String>) {
    let mut media_paths: Vec<String> = Vec::new();
    let mut text_lines: Vec<String> = Vec::new();

    for line in reply.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(MEDIA_PREFIX) {
            let path = rest.trim();
            if !path.is_empty() {
                media_paths.push(path.to_string());
            }
            continue;
        }
        if trimmed.starts_with(MEDIA_URI_SCHEME) {
            let rel = trimmed
                .strip_prefix(MEDIA_URI_SCHEME)
                .unwrap_or(trimmed)
                .trim();
            if !rel.is_empty() {
                media_paths.push(rel.to_string());
            }
            continue;
        }

        let mut line_text = line.to_string();
        for cap in inline_media_re().captures_iter(line) {
            if let Some(m) = cap.get(1) {
                let path = m.as_str().trim();
                if !path.is_empty() {
                    media_paths.push(path.to_string());
                }
            }
        }
        line_text = inline_media_re().replace_all(&line_text, "").to_string();
        if !line_text.trim().is_empty() {
            text_lines.push(line_text);
        }
    }

    let text = text_lines.join("\n");
    (text, media_paths)
}

/// Remove `MEDIA:` / `pointer-media://` markers for App UI display (IM dispatch uses `split_reply_media`).
pub fn strip_outbound_media_markers(text: &str) -> String {
    split_reply_media(text).0.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_inline_media_token() {
        let (text, media) = split_reply_media("Hello MEDIA:/tmp/a.png world");
        assert!(text.contains("Hello"));
        assert!(text.contains("world"));
        assert_eq!(media, vec!["/tmp/a.png"]);
    }

    #[test]
    fn strips_media_lines() {
        let (text, media) = split_reply_media("Hello\nMEDIA:/tmp/a.png\nWorld");
        assert_eq!(text, "Hello\nWorld");
        assert_eq!(media, vec!["/tmp/a.png"]);
    }

    #[test]
    fn strip_for_app_ui() {
        let out = strip_outbound_media_markers(
            "找到了桌面上的 baby_cover.jpg，发给你 👇\n\nMEDIA:/Users/starliu/Desktop/baby_cover.jpg",
        );
        assert!(!out.contains("MEDIA:"));
        assert!(!out.contains("/Users/starliu/Desktop"));
        assert!(out.contains("发给你"));
        assert!(out.contains("baby_cover.jpg"));
    }
}
