use serde_json::Value;

/// Remove structured tool JSON (or legacy XML) from assistant `content` for the user-visible bubble.
pub(crate) fn extract_user_visible_content(raw: &str) -> String {
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
            return String::new();
        }
    }
    extract_user_visible_content_xml_legacy(raw)
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
}
