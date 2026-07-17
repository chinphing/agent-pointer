use crate::media::outbound_reply::{split_reply_media, strip_outbound_media_markers};
use crate::media::attachments_from_reply_paths;
use crate::models::MediaAttachment;
use serde_json::Value;

/// Remove structured tool JSON (or legacy XML) from assistant `content` for the user-visible bubble.
pub(crate) fn extract_user_visible_content(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if let Value::Object(ref obj) = v {
            if obj.get("tool_name").and_then(|x| x.as_str()) == Some("response") {
                let visible = obj
                    .get("tool_args")
                    .and_then(|a| a.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                return strip_outbound_media_markers(visible);
            }
            return String::new();
        }
    }
    strip_outbound_media_markers(&extract_user_visible_content_xml_legacy(raw))
}

/// `MEDIA:` paths from assistant raw stream buffer (plain text or `response` tool JSON).
pub(crate) fn reply_attachments_from_assistant_raw(raw: &str) -> Option<Vec<MediaAttachment>> {
    let media_source = assistant_raw_media_source_owned(raw);
    let (_, paths) = split_reply_media(&media_source);
    if paths.is_empty() {
        None
    } else {
        let atts = attachments_from_reply_paths(&paths);
        if atts.is_empty() {
            None
        } else {
            Some(atts)
        }
    }
}

fn assistant_raw_media_source_owned(raw: &str) -> String {
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

fn extract_user_visible_content_xml_legacy(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut remaining = raw;

    while let Some(start) = remaining.find("<response>") {
        if start > 0 {
            result.push_str(&remaining[..start]);
        }
        if let Some(end) = remaining[start..].find("</response>") {
            remaining = &remaining[start + end + 11..];
        } else {
            result.push_str(&remaining[start..]);
            break;
        }
    }

    if !remaining.is_empty() {
        result.push_str(remaining);
    }

    result.trim().to_string()
}

#[cfg(test)]
mod extract_user_visible_tests {
    use super::extract_user_visible_content;

    #[test]
    fn json_response_tool_text_visible() {
        let j = r#"{"thoughts":"t","headline":"h","tool_name":"response","tool_args":{"text":"Hello user"}}"#;
        assert_eq!(extract_user_visible_content(j), "Hello user");
    }

    #[test]
    fn json_non_response_hidden() {
        let j = r#"{"tool_name":"wait","tool_args":{"seconds":"1"}}"#;
        assert_eq!(extract_user_visible_content(j), "");
    }

    #[test]
    fn legacy_xml_only_response_still_stripped() {
        assert_eq!(
            extract_user_visible_content("<response><tool_name>x</tool_name></response>"),
            ""
        );
    }

    #[test]
    fn legacy_prose_outside_response_kept() {
        assert_eq!(
            extract_user_visible_content("Hi<response></response>"),
            "Hi"
        );
    }

    #[test]
    fn strips_media_markers_from_prose_when_file_missing() {
        let raw = "发给你 👇\n\nMEDIA:/Users/me/Desktop/baby_cover.jpg";
        let out = extract_user_visible_content(raw);
        assert!(out.contains("MEDIA:"));
        assert!(out.contains("发给你"));
    }

    #[test]
    fn plain_text_media_skips_attachment_when_file_missing() {
        use super::reply_attachments_from_assistant_raw;
        let raw = "好的，再发一次 👇\n\nMEDIA:/Users/me/Desktop/baby_cover.jpg";
        assert!(reply_attachments_from_assistant_raw(raw).is_none());
        assert_eq!(
            extract_user_visible_content(raw),
            "好的，再发一次 👇\nMEDIA:/Users/me/Desktop/baby_cover.jpg"
        );
    }

    /// Repro: customer sandbox empty.txt via `response` tool JSON + backtick MEDIA.
    #[test]
    fn repro_response_tool_sandbox_empty_txt_attaches() {
        use super::reply_attachments_from_assistant_raw;
        use std::fs;

        let sandbox = std::env::temp_dir()
            .join(format!("PointerApp-repro-{}", uuid::Uuid::new_v4()))
            .join("session-sandboxes")
            .join("1530c681-176d-40ca-84b4-a90a34312628");
        let file = sandbox.join("empty.txt");
        fs::create_dir_all(&sandbox).unwrap();
        fs::write(&file, b"").unwrap();
        let path = file.display().to_string();

        let text = format!(
            "已创建空文件，内容为 0 字节：\nMEDIA:`{path}`\n如果你想换个文件名或目录，随时告诉我！"
        );
        let raw = serde_json::json!({
            "tool_name": "response",
            "tool_args": { "text": text },
        })
        .to_string();

        let atts = reply_attachments_from_assistant_raw(&raw)
            .expect("should build attachments when sandbox empty.txt exists");
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].file_name, "empty.txt");

        let visible = extract_user_visible_content(&raw);
        assert!(
            !visible.contains("MEDIA:"),
            "visible bubble should strip resolved MEDIA; got:\n{visible}"
        );
        assert!(visible.contains("已创建空文件"));

        let _ = fs::remove_file(&file);
        let _ = fs::remove_dir_all(
            sandbox
                .parent()
                .and_then(|p| p.parent())
                .unwrap_or(&sandbox),
        );
    }
}
