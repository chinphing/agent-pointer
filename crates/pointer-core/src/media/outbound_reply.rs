//! Parse outbound media markers from agent replies (OpenClaw `MEDIA:` convention).

use super::path_hint::MEDIA_URI_SCHEME;
use super::resolve::resolve_local_media_path;
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

/// True when a `MEDIA:` path reference resolves to an existing file on disk.
pub fn reply_media_path_resolves(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }
    resolve_local_media_path(trimmed)
        .ok()
        .is_some_and(|p| p.is_file())
}

fn strip_resolved_inline_media(line: &str, media_paths: &mut Vec<String>) -> String {
    inline_media_re()
        .replace_all(line, |caps: &regex::Captures| {
            let Some(m) = caps.get(1) else {
                return caps.get(0).map(|x| x.as_str()).unwrap_or("").to_string();
            };
            let path = m.as_str().trim();
            if path.is_empty() {
                return caps.get(0).map(|x| x.as_str()).unwrap_or("").to_string();
            }
            if reply_media_path_resolves(path) {
                media_paths.push(path.to_string());
                String::new()
            } else {
                caps.get(0).map(|x| x.as_str()).unwrap_or("").to_string()
            }
        })
        .to_string()
}

/// Split agent reply into user-visible text and resolvable local media path references.
///
/// `MEDIA:` markers are removed from visible text **only** when the referenced file exists.
/// Unresolvable paths stay in the visible body so delivery failures remain visible.
pub fn split_reply_media(reply: &str) -> (String, Vec<String>) {
    let mut media_paths: Vec<String> = Vec::new();
    let mut text_lines: Vec<String> = Vec::new();

    for line in reply.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(MEDIA_PREFIX) {
            let path = rest.trim();
            if path.is_empty() {
                text_lines.push(line.to_string());
                continue;
            }
            if reply_media_path_resolves(path) {
                media_paths.push(path.to_string());
            } else {
                text_lines.push(line.to_string());
            }
            continue;
        }
        // Bare `pointer-media://` lines are inbound attachment hints — hide from text but
        // do not inline them as assistant outbound media (only explicit `MEDIA:` counts).
        if trimmed.starts_with(MEDIA_URI_SCHEME) {
            continue;
        }

        let line_text = strip_resolved_inline_media(line, &mut media_paths);
        if !line_text.trim().is_empty() {
            text_lines.push(line_text);
        }
    }

    let text = text_lines.join("\n");
    (text, media_paths)
}

/// Remove resolved `MEDIA:` / bare `pointer-media://` markers for App UI display.
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

/// Rebuild IM outbound reply text: user-visible body plus resolved `MEDIA:` lines from raw output.
pub fn im_outbound_reply_source(raw_content: Option<&str>, visible_content: Option<&str>) -> String {
    let Some(raw) = raw_content.map(str::trim).filter(|s| !s.is_empty()) else {
        return visible_content.unwrap_or("").trim().to_string();
    };

    let source = reply_media_source(raw);
    let (_, resolved_paths) = split_reply_media(&source);
    if resolved_paths.is_empty() {
        return visible_content.unwrap_or(raw).trim().to_string();
    }

    let visible = visible_content
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| split_reply_media(&source).0.trim().to_string());

    let mut out = visible;
    for path in resolved_paths {
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
    use std::fs;
    use std::path::PathBuf;

    fn touch(path: &PathBuf) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(path, b"test").unwrap();
    }

    #[test]
    fn strips_inline_media_token_when_file_exists() {
        let file = std::env::temp_dir().join(format!(
            "pointer-outbound-inline-{}.png",
            uuid::Uuid::new_v4()
        ));
        touch(&file);
        let path = file.display().to_string();
        let (text, media) = split_reply_media(&format!("Hello MEDIA:{path} world"));
        assert!(text.contains("Hello"));
        assert!(text.contains("world"));
        assert!(!text.contains("MEDIA:"));
        assert_eq!(media, vec![path]);
        let _ = fs::remove_file(&file);
    }

    #[test]
    fn keeps_inline_media_token_when_file_missing() {
        let path = format!(
            "/tmp/pointer-outbound-missing-inline-{}.png",
            uuid::Uuid::new_v4()
        );
        let (text, media) = split_reply_media(&format!("Hello MEDIA:{path} world"));
        assert!(text.contains("MEDIA:"));
        assert!(text.contains(&path));
        assert!(media.is_empty());
    }

    #[test]
    fn strips_media_lines_when_file_exists() {
        let file = std::env::temp_dir().join(format!(
            "pointer-outbound-line-{}.png",
            uuid::Uuid::new_v4()
        ));
        touch(&file);
        let path = file.display().to_string();
        let (text, media) = split_reply_media(&format!("Hello\nMEDIA:{path}\nWorld"));
        assert_eq!(text, "Hello\nWorld");
        assert_eq!(media, vec![path]);
        let _ = fs::remove_file(&file);
    }

    #[test]
    fn keeps_media_line_when_file_missing() {
        let path = format!(
            "/tmp/pointer-outbound-missing-line-{}.png",
            uuid::Uuid::new_v4()
        );
        let (text, media) = split_reply_media(&format!("Hello\nMEDIA:{path}\nWorld"));
        assert_eq!(text, format!("Hello\nMEDIA:{path}\nWorld"));
        assert!(media.is_empty());
    }

    #[test]
    fn strip_for_app_ui_keeps_unresolved_media_line() {
        let out = strip_outbound_media_markers(
            "找到了桌面上的 baby_cover.jpg，发给你 👇\n\nMEDIA:/Users/starliu/Desktop/baby_cover.jpg",
        );
        assert!(out.contains("MEDIA:"));
        assert!(out.contains("/Users/starliu/Desktop/baby_cover.jpg"));
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
    fn media_prefix_pointer_uri_is_outbound_when_file_exists() {
        let file = std::env::temp_dir().join(format!(
            "pointer-outbound-uri-{}.wav",
            uuid::Uuid::new_v4()
        ));
        touch(&file);
        let uri = format!("file://{}", file.display());
        let (text, media) = split_reply_media(&format!("好的\nMEDIA:{uri}"));
        assert_eq!(media, vec![uri]);
        assert!(!text.contains("MEDIA:"));
        let _ = fs::remove_file(&file);
    }

    #[test]
    fn im_outbound_reply_source_restores_resolved_media_from_raw() {
        let file = std::env::temp_dir().join(format!(
            "pointer-outbound-im-{}.html",
            uuid::Uuid::new_v4()
        ));
        touch(&file);
        let path = file.display().to_string();
        let raw = format!("文件在这里 👇\n\nMEDIA:{path}");
        let visible = strip_outbound_media_markers(&raw);
        let outbound = im_outbound_reply_source(Some(&raw), Some(&visible));
        let (text, media) = split_reply_media(&outbound);
        assert_eq!(text, visible);
        assert_eq!(media, vec![path]);
        let _ = fs::remove_file(&file);
    }
}
