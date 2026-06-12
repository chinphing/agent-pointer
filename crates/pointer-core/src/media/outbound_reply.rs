//! Parse outbound media markers from agent replies (OpenClaw `MEDIA:` convention).

use super::path_hint::MEDIA_URI_SCHEME;
use regex::Regex;
use serde_json::Value;
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
        // Bare `pointer-media://` lines are inbound attachment hints — hide from text but
        // do not inline them as assistant outbound media (only explicit `MEDIA:` counts).
        if trimmed.starts_with(MEDIA_URI_SCHEME) {
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

/// Assistant raw output slice that may contain `MEDIA:` (plain text or response-tool JSON).
pub fn reply_media_source(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if let Value::Object(ref obj) = v {
            if obj.get("tool_name").and_then(|x| x.as_str()) == Some("response") {
                return obj
                    .get("tool_args")
                    .and_then(|a| a.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
            }
        }
    }
    raw.to_string()
}

/// Rebuild IM outbound reply text: user-visible body plus `MEDIA:` lines from raw assistant output.
pub fn im_outbound_reply_source(raw_content: Option<&str>, visible_content: Option<&str>) -> String {
    let Some(raw) = raw_content.map(str::trim).filter(|s| !s.is_empty()) else {
        return visible_content.unwrap_or("").trim().to_string();
    };

    let source = reply_media_source(raw);
    let (_, media_paths) = split_reply_media(&source);
    if media_paths.is_empty() {
        return visible_content.unwrap_or(raw).trim().to_string();
    }

    let visible = visible_content
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| split_reply_media(&source).0.trim().to_string());

    let mut out = visible;
    for path in media_paths {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(MEDIA_PREFIX);
        out.push_str(&path);
    }
    out
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

    #[test]
    fn bare_pointer_media_line_is_not_outbound_media() {
        let uri = "pointer-media://conv-id/27582151-a370-4170-873b-bc12b9bff2c5.wav";
        let (text, media) = split_reply_media(&format!("{uri}\n当然可以！我能帮你写代码。"));
        assert!(media.is_empty());
        assert!(text.contains("当然可以"));
        assert!(!text.contains("pointer-media://"));
    }

    #[test]
    fn media_prefix_pointer_uri_is_outbound() {
        let uri = "pointer-media://conv-id/att.wav";
        let (text, media) = split_reply_media(&format!("好的\nMEDIA:{uri}"));
        assert_eq!(media, vec![uri]);
        assert!(!text.contains("pointer-media://"));
    }

    #[test]
    fn im_outbound_reply_source_restores_media_from_raw() {
        let raw = "文件在这里 👇\n\nMEDIA:/tmp/minesweeper.html";
        let visible = strip_outbound_media_markers(raw);
        let outbound = im_outbound_reply_source(Some(raw), Some(&visible));
        let (text, media) = split_reply_media(&outbound);
        assert_eq!(text, visible);
        assert_eq!(media, vec!["/tmp/minesweeper.html"]);
    }
}
