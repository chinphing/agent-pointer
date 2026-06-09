//! Parse outbound media markers from agent replies (OpenClaw `MEDIA:` convention).

use pointer_core::media::path_hint::MEDIA_URI_SCHEME;
use regex::Regex;
use std::sync::OnceLock;

const MEDIA_PREFIX: &str = "MEDIA:";

fn media_token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\bMEDIA:\s*`?([^\n`]+)`?").expect("media token re"))
}

/// Split agent reply into user-visible text and local media path references.
pub fn split_reply_media(reply: &str) -> (String, Vec<String>) {
    let mut media_paths: Vec<String> = Vec::new();
    let mut text = reply.to_string();

    for cap in media_token_re().captures_iter(reply) {
        if let Some(m) = cap.get(1) {
            let path = m.as_str().trim();
            if !path.is_empty() {
                media_paths.push(path.to_string());
            }
        }
    }

    text = media_token_re().replace_all(&text, "").to_string();
    for line in reply.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(MEDIA_URI_SCHEME) {
            let rel = trimmed
                .strip_prefix(MEDIA_URI_SCHEME)
                .unwrap_or(trimmed)
                .trim();
            if !rel.is_empty() && !media_paths.iter().any(|p| p == rel) {
                media_paths.push(rel.to_string());
            }
        }
    }

    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    let text = text.trim().to_string();

    (text, media_paths)
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
        assert_eq!(text, "Hello\n\nWorld");
        assert_eq!(media, vec!["/tmp/a.png"]);
    }

    #[test]
    fn accepts_pointer_media_uri() {
        let (text, media) = split_reply_media(
            "Done\npointer-media://conv-1/att.jpg",
        );
        assert_eq!(text, "Done");
        assert_eq!(media, vec!["conv-1/att.jpg"]);
    }

    #[test]
    fn media_only_reply() {
        let (text, media) = split_reply_media("MEDIA:conv/x.pdf");
        assert!(text.is_empty());
        assert_eq!(media, vec!["conv/x.pdf"]);
    }
}
