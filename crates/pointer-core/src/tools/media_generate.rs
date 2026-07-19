//! `image_generate` and `video_generate` agent tools.

mod dispatch;

pub use dispatch::{dispatch_media_generate_async, MediaGenerateDispatchContext};

use super::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::Arc;

const IMAGE_DOC: &str = include_str!("prompts/image_generate.md");
const VIDEO_DOC: &str = include_str!("prompts/video_generate.md");

fn parse_prompt(args: &Value) -> Result<String> {
    args.get("prompt")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("missing or empty prompt"))
}

fn parse_optional_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn parse_count(args: &Value) -> u32 {
    args.get("count")
        .and_then(|v| v.as_u64())
        .map(|n| n.clamp(1, 4) as u32)
        .unwrap_or(1)
}

fn parse_duration(args: &Value) -> Option<u32> {
    args.get("durationSeconds")
        .or_else(|| args.get("duration_seconds"))
        .and_then(|v| v.as_u64())
        .map(|n| n.clamp(2, 15) as u32)
}

fn parse_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|v| v.as_bool())
}

pub fn build_image_request(args: &Value) -> Result<crate::media_generation::ImageGenerateRequest> {
    Ok(crate::media_generation::ImageGenerateRequest {
        prompt: parse_prompt(args)?,
        size: parse_optional_string(args, "size"),
        n: parse_count(args),
        image_url: parse_optional_string(args, "image").or_else(|| parse_optional_string(args, "imageUrl")),
    })
}

pub fn build_video_request(args: &Value) -> Result<crate::media_generation::VideoGenerateRequest> {
    Ok(crate::media_generation::VideoGenerateRequest {
        prompt: parse_prompt(args)?,
        size: parse_optional_string(args, "size").or_else(|| parse_optional_string(args, "resolution")),
        duration_seconds: parse_duration(args),
        image_url: parse_optional_string(args, "image").or_else(|| parse_optional_string(args, "imageUrl")),
        audio: parse_bool(args, "audio"),
    })
}

fn stub_handler() -> ToolHandler {
    Arc::new(|_| {
        Err(anyhow!(
            "media generation tools run on the async chat runtime path"
        ))
    })
}

pub fn register_all(reg: &ToolRegistry) {
    reg.register(
        ToolEntry::new(
            "image_generate",
            "tools/prompts/image_generate.md",
            "medium",
            true,
            IMAGE_DOC.trim(),
            stub_handler(),
        )
        .with_final_reply(true)
        .with_subagent_inheritance(false),
    );
    reg.register(
        ToolEntry::new(
            "video_generate",
            "tools/prompts/video_generate.md",
            "high",
            true,
            VIDEO_DOC.trim(),
            stub_handler(),
        )
        .with_final_reply(true)
        .with_subagent_inheritance(false),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_image_args() {
        let req = build_image_request(&json!({
            "prompt": "a cat",
            "size": "2K",
            "count": 2
        }))
        .unwrap();
        assert_eq!(req.prompt, "a cat");
        assert_eq!(req.size.as_deref(), Some("2K"));
        assert_eq!(req.n, 2);
    }
}
