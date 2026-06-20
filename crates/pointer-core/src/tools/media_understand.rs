//! `media_understand` agent tool — on-demand image/video/audio/PDF understanding.

mod dispatch;

pub use dispatch::{dispatch_media_understand_async, MediaUnderstandDispatchContext};

use super::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::Arc;

const DOC: &str = include_str!("prompts/media_understand.md");

pub fn parse_ref_and_mode(args: &Value) -> Result<(String, String)> {
    let media_ref = args
        .get("ref")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("missing or empty ref"))?;
    let mode = args
        .get("mode")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .ok_or_else(|| anyhow!("missing or empty mode"))?;
    match mode.as_str() {
        "image" | "video" | "audio" | "pdf" => Ok((media_ref, mode)),
        _ => Err(anyhow!("mode must be image, video, audio, or pdf")),
    }
}

pub fn parse_goal(args: &Value) -> Result<String> {
    args.get("goal")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("missing or empty goal"))
}

pub fn parse_context(args: &Value) -> Option<String> {
    args.get("context")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Combine required goal with optional context for model prompts.
pub fn format_goal_block(goal: &str, context: Option<&str>) -> String {
    match context.filter(|s| !s.trim().is_empty()) {
        Some(ctx) => format!("{goal}\n\nAdditional context:\n{ctx}"),
        None => goal.to_string(),
    }
}

fn stub_handler() -> ToolHandler {
    Arc::new(|_| {
        Err(anyhow!(
            "media_understand runs on the async chat runtime path"
        ))
    })
}

pub fn register_all(reg: &ToolRegistry) {
    reg.register(ToolEntry::new(
        "media_understand",
        "tools/prompts/media_understand.md",
        "medium",
        false,
        DOC.trim(),
        stub_handler(),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::json_schema_from_markdown;
    use serde_json::json;

    #[test]
    fn doc_has_valid_schema_frontmatter() {
        let schema = json_schema_from_markdown(DOC).expect("media_understand.md schema");
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["ref"].is_object());
        assert!(schema["properties"]["mode"].is_object());
        assert!(schema["properties"]["goal"].is_object());
        assert!(schema["required"].as_array().unwrap().contains(&json!("goal")));
    }

    #[test]
    fn parse_ref_and_mode_ok() {
        let args = json!({
            "ref": "pointer-media://c/a.png",
            "mode": "image",
            "goal": "Describe visible UI elements"
        });
        let (r, m) = parse_ref_and_mode(&args).unwrap();
        assert_eq!(r, "pointer-media://c/a.png");
        assert_eq!(m, "image");
    }

    #[test]
    fn parse_goal_required() {
        let args = json!({"ref": "x", "mode": "pdf"});
        assert!(parse_goal(&args).is_err());
        let args = json!({"ref": "x", "mode": "pdf", "goal": "Summarize key terms"});
        assert_eq!(parse_goal(&args).unwrap(), "Summarize key terms");
    }

    #[test]
    fn format_goal_block_merges_context() {
        let block = format_goal_block("Summarize", Some("User asked in Chinese"));
        assert!(block.contains("Summarize"));
        assert!(block.contains("Additional context"));
    }

    #[test]
    fn parse_ref_and_mode_rejects_bad_mode() {
        let args = json!({"ref": "x", "mode": "zip"});
        assert!(parse_ref_and_mode(&args).is_err());
    }
}
