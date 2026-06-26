//! `media_understand` agent tool — on-demand image/video/audio/PDF understanding.

mod dispatch;

pub use dispatch::{dispatch_media_understand_async, MediaUnderstandDispatchContext};

use super::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::Arc;

const DOC: &str = include_str!("prompts/media_understand.md");

pub fn parse_mode(args: &Value) -> Result<String> {
    let mode = args
        .get("mode")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .ok_or_else(|| anyhow!("missing or empty mode"))?;
    match mode.as_str() {
        "image" | "video" | "audio" | "pdf" => Ok(mode),
        _ => Err(anyhow!("mode must be image, video, audio, or pdf")),
    }
}

pub fn parse_refs(args: &Value, mode: &str) -> Result<Vec<String>> {
    use crate::media::image_dir::MAX_IMAGES_PER_CALL;

    if args.get("ref").is_some() {
        return Err(anyhow!(
            "media_understand: parameter ref was removed; use refs (array). For {mode}, pass exactly one element."
        ));
    }

    let refs: Vec<String> = match args.get("refs").and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|v| {
                v.as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
            })
            .collect(),
        None => Vec::new(),
    };
    if refs.is_empty() {
        anyhow::bail!("missing or empty refs");
    }

    match mode {
        "image" => {
            if refs.len() > MAX_IMAGES_PER_CALL {
                anyhow::bail!(
                    "requested {} images via refs; max {MAX_IMAGES_PER_CALL} per call — split into multiple media_understand calls",
                    refs.len()
                );
            }
            Ok(refs)
        }
        "audio" | "video" | "pdf" => {
            if refs.len() > 1 {
                anyhow::bail!(
                    "mode={mode} accepts only one ref; pass refs with exactly one element (got {})",
                    refs.len()
                );
            }
            Ok(refs)
        }
        other => Err(anyhow!("unsupported media_understand mode: {other}")),
    }
}

pub fn parse_single_media_ref(args: &Value, mode: &str) -> Result<String> {
    parse_refs(args, mode)?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("missing refs"))
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

/// Parse optional 1-based PDF page range. Default: pages 1–10 when user did not specify.
pub fn parse_pdf_page_range(args: &Value, total_pages: usize) -> Result<crate::media::PdfPageRange> {
    use crate::media::pdf::{PdfPageRange, DEFAULT_PDF_PAGE_END};

    let start = args
        .get("pageStart")
        .and_then(|v| v.as_u64())
        .map(|n| usize::try_from(n).unwrap_or(0));
    let end = args
        .get("pageEnd")
        .and_then(|v| v.as_u64())
        .map(|n| usize::try_from(n).unwrap_or(0));

    let range = match (start, end) {
        (None, None) => PdfPageRange::default_first_window(total_pages)?,
        (Some(s), None) => PdfPageRange::normalize(total_pages, s, total_pages.min(s + DEFAULT_PDF_PAGE_END - 1))?,
        (None, Some(e)) => PdfPageRange::normalize(total_pages, 1, e)?,
        (Some(s), Some(e)) => PdfPageRange::normalize(total_pages, s, e)?,
    };
    range.ensure_within_per_call_limit()?;
    Ok(range)
}

/// Default video window: first segment at 1 frame/second (max 200 frames).
pub fn parse_video_time_range(_args: &Value, duration_sec: f64) -> Result<crate::media::VideoTimeRange> {
    use crate::media::video::{VideoTimeRange, DEFAULT_FRAMES_PER_SECOND};

    let range = VideoTimeRange::default_first_window(duration_sec.max(0.0), DEFAULT_FRAMES_PER_SECOND)?;
    range.ensure_within_per_call_limit()?;
    Ok(range)
}

/// Parse 1-based image index range for directory refs via **pageStart** / **pageEnd**.
/// Default: images 1–200 when unspecified.
pub fn parse_image_dir_range(args: &Value, total_images: usize) -> Result<crate::media::ImageDirRange> {
    use crate::media::image_dir::{ImageDirRange, DEFAULT_IMAGE_BATCH};

    let start = args
        .get("pageStart")
        .and_then(|v| v.as_u64())
        .map(|n| usize::try_from(n).unwrap_or(0));
    let end = args
        .get("pageEnd")
        .and_then(|v| v.as_u64())
        .map(|n| usize::try_from(n).unwrap_or(0));

    let range = match (start, end) {
        (None, None) => ImageDirRange::default_first_batch(total_images)?,
        (Some(s), None) => {
            ImageDirRange::normalize(total_images, s, total_images.min(s + DEFAULT_IMAGE_BATCH - 1))?
        }
        (None, Some(e)) => ImageDirRange::normalize(total_images, 1, e)?,
        (Some(s), Some(e)) => ImageDirRange::normalize(total_images, s, e)?,
    };
    range.ensure_within_per_call_limit()?;
    Ok(range)
}

pub(crate) fn prepend_scope_notice(body: &str, notice: &str) -> String {
    if body.trim().is_empty() {
        notice.to_string()
    } else {
        format!("{notice}\n\n{body}")
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
        assert!(schema["properties"]["refs"].is_object());
        assert!(schema["properties"]["mode"].is_object());
        assert!(schema["properties"]["goal"].is_object());
        assert!(!schema["properties"].as_object().unwrap().contains_key("ref"));
        assert!(!schema["properties"].as_object().unwrap().contains_key("imageStart"));
        assert!(!schema["properties"].as_object().unwrap().contains_key("timeStartSec"));
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("goal")));
        assert!(required.contains(&json!("mode")));
        assert!(required.contains(&json!("refs")));
    }

    #[test]
    fn parse_refs_image_single() {
        let args = json!({
            "refs": ["pointer-media://c/a.png"],
            "mode": "image",
            "goal": "Describe visible UI elements"
        });
        let refs = parse_refs(&args, "image").unwrap();
        assert_eq!(refs, vec!["pointer-media://c/a.png"]);
    }

    #[test]
    fn parse_goal_required() {
        let args = json!({"refs": ["x"], "mode": "pdf"});
        assert!(parse_goal(&args).is_err());
        let args = json!({"refs": ["x"], "mode": "pdf", "goal": "Summarize key terms"});
        assert_eq!(parse_goal(&args).unwrap(), "Summarize key terms");
    }

    #[test]
    fn format_goal_block_merges_context() {
        let block = format_goal_block("Summarize", Some("User asked in Chinese"));
        assert!(block.contains("Summarize"));
        assert!(block.contains("Additional context"));
    }

    #[test]
    fn parse_refs_image_multi() {
        let args = json!({
            "refs": [
                "pointer-media://c/a.png",
                "pointer-media://c/b.jpg"
            ],
            "mode": "image",
            "goal": "Compare layouts"
        });
        let refs = parse_refs(&args, "image").unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0], "pointer-media://c/a.png");
    }

    #[test]
    fn parse_refs_rejects_legacy_ref_field() {
        let args = json!({
            "ref": "pointer-media://c/a.png",
            "mode": "image",
            "goal": "Describe"
        });
        assert!(parse_refs(&args, "image").is_err());
    }

    #[test]
    fn parse_refs_image_rejects_too_many() {
        let refs: Vec<String> = (0..201).map(|i| format!("pointer-media://c/{i}.png")).collect();
        let args = json!({
            "refs": refs,
            "mode": "image",
            "goal": "batch"
        });
        assert!(parse_refs(&args, "image").is_err());
    }

    #[test]
    fn parse_refs_pdf_accepts_one() {
        let args = json!({
            "refs": ["pointer-media://c/a.pdf"],
            "mode": "pdf",
            "goal": "read"
        });
        assert_eq!(
            parse_single_media_ref(&args, "pdf").unwrap(),
            "pointer-media://c/a.pdf"
        );
    }

    #[test]
    fn parse_refs_pdf_rejects_multiple() {
        let args = json!({
            "refs": ["pointer-media://c/a.pdf", "pointer-media://c/b.pdf"],
            "mode": "pdf",
            "goal": "read"
        });
        let err = parse_refs(&args, "pdf").unwrap_err().to_string();
        assert!(err.contains("only one ref"));
    }

    #[test]
    fn parse_mode_rejects_bad_mode() {
        let args = json!({"refs": ["x"], "mode": "zip", "goal": "x"});
        assert!(parse_mode(&args).is_err());
    }

    #[test]
    fn parse_pdf_page_range_defaults_to_first_ten() {
        let args = json!({"refs": ["x"], "mode": "pdf", "goal": "summarize"});
        let range = parse_pdf_page_range(&args, 100).unwrap();
        assert_eq!(range.start, 1);
        assert_eq!(range.end, 10);
        assert!(!range.user_specified);
    }

    #[test]
    fn parse_pdf_page_range_user_range() {
        let args = json!({
            "refs": ["x"],
            "mode": "pdf",
            "goal": "read",
            "pageStart": 5,
            "pageEnd": 8
        });
        let range = parse_pdf_page_range(&args, 100).unwrap();
        assert_eq!(range.start, 5);
        assert_eq!(range.end, 8);
        assert!(range.user_specified);
    }

    #[test]
    fn parse_pdf_page_range_rejects_too_many_pages() {
        let args = json!({
            "refs": ["x"],
            "mode": "pdf",
            "goal": "read",
            "pageStart": 1,
            "pageEnd": 20
        });
        assert!(parse_pdf_page_range(&args, 100).is_err());
    }

    #[test]
    fn parse_video_time_range_defaults_first_segment_at_one_fps() {
        let args = json!({"refs": ["x"], "mode": "video", "goal": "watch"});
        let range = parse_video_time_range(&args, 500.0).unwrap();
        assert_eq!(range.start_sec, 0.0);
        assert_eq!(range.end_sec, 200.0);
        assert_eq!(range.frames_per_second, 1.0);
        assert!(!range.user_specified_time);
        assert_eq!(range.frame_count(), 200);
    }

    #[test]
    fn parse_video_time_range_short_video_uses_full_duration() {
        let args = json!({"refs": ["x"], "mode": "video", "goal": "watch"});
        let range = parse_video_time_range(&args, 30.0).unwrap();
        assert_eq!(range.end_sec, 30.0);
        assert_eq!(range.frame_count(), 30);
    }

    #[test]
    fn parse_video_time_range_ignores_legacy_time_params() {
        let args = json!({
            "refs": ["x"],
            "mode": "video",
            "goal": "watch",
            "timeStartSec": 10.0,
            "timeEndSec": 20.0,
            "framesPerSecond": 2.0
        });
        let range = parse_video_time_range(&args, 120.0).unwrap();
        assert_eq!(range.start_sec, 0.0);
        assert_eq!(range.end_sec, 120.0);
        assert_eq!(range.frames_per_second, 1.0);
        assert!(!range.user_specified_time);
    }

    #[test]
    fn parse_image_dir_range_defaults_first_batch() {
        let args = json!({"refs": ["/photos"], "mode": "image", "goal": "describe"});
        let range = parse_image_dir_range(&args, 500).unwrap();
        assert_eq!(range.start, 1);
        assert_eq!(range.end, 200);
        assert!(!range.user_specified);
    }

    #[test]
    fn parse_image_dir_range_user_page_window() {
        let args = json!({
            "refs": ["/photos"],
            "mode": "image",
            "goal": "describe",
            "pageStart": 5,
            "pageEnd": 8
        });
        let range = parse_image_dir_range(&args, 50).unwrap();
        assert_eq!(range.start, 5);
        assert_eq!(range.end, 8);
        assert!(range.user_specified);
    }

    #[test]
    fn parse_image_dir_range_rejects_too_many() {
        let args = json!({
            "refs": ["/photos"],
            "mode": "image",
            "goal": "describe",
            "pageStart": 1,
            "pageEnd": 250
        });
        assert!(parse_image_dir_range(&args, 500).is_err());
    }
}
