//! Verify-phase wire helpers (debug text for pipeline UI).

use serde_json::Value;

pub use super::wire_format::{build_verify_wire_messages, VerifyWireInput, TAG_VERIFY};

/// Text-only debug view of pipeline wire messages (images replaced with placeholders).
pub fn wire_messages_debug_text(messages: &[Value]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for msg in messages {
        let Some(role) = msg.get("role").and_then(|v| v.as_str()) else {
            continue;
        };
        let content = msg.get("content");
        match content {
            Some(Value::String(s)) => {
                parts.push(format!("{role}:\n{}", s.trim()));
            }
            Some(Value::Array(items)) => {
                let mut block = format!("{role}:");
                for item in items {
                    let Some(obj) = item.as_object() else {
                        continue;
                    };
                    match obj.get("type").and_then(|v| v.as_str()) {
                        Some("text") => {
                            if let Some(t) = obj.get("text").and_then(|v| v.as_str()) {
                                block.push('\n');
                                block.push_str(t.trim_end());
                            }
                        }
                        Some("image_url") => {
                            block.push_str("\n[image attached]");
                        }
                        _ => {}
                    }
                }
                parts.push(block);
            }
            _ => {}
        }
    }
    parts.join("\n\n")
}
