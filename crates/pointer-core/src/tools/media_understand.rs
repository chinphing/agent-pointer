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

/// Parse video time window and sampling. Default: first segment at 1 frame/second (max 200 frames).
pub fn parse_video_time_range(args: &Value, duration_sec: f64) -> Result<crate::media::VideoTimeRange> {
    use crate::media::video::{VideoTimeRange, DEFAULT_FRAMES_PER_SECOND};

    let has_start = args.get("timeStartSec").is_some();
    let has_end = args.get("timeEndSec").is_some();
    let fps = args
        .get("framesPerSecond")
        .and_then(|v| v.as_f64())
        .filter(|n| n.is_finite() && *n > 0.0)
        .unwrap_or(DEFAULT_FRAMES_PER_SECOND);
    let max_window = VideoTimeRange::max_window_sec_for_fps(fps);
    let duration = duration_sec.max(0.0);

    let range = match (has_start, has_end) {
        (false, false) => VideoTimeRange::default_first_window(duration, fps)?,
        (true, false) => {
            let start = args
                .get("timeStartSec")
                .and_then(|v| v.as_f64())
                .filter(|n| n.is_finite() && *n >= 0.0)
                .unwrap_or(0.0);
            let end = (start + max_window).min(duration);
            VideoTimeRange::normalize(duration, start, end, fps)?
        }
        (false, true) => {
            let end = args
                .get("timeEndSec")
                .and_then(|v| v.as_f64())
                .filter(|n| n.is_finite() && *n >= 0.0)
                .unwrap_or(duration);
            VideoTimeRange::normalize(duration, 0.0, end, fps)?
        }
        (true, true) => {
            let start = args
                .get("timeStartSec")
                .and_then(|v| v.as_f64())
                .filter(|n| n.is_finite() && *n >= 0.0)
                .unwrap_or(0.0);
            let end = args
                .get("timeEndSec")
                .and_then(|v| v.as_f64())
                .filter(|n| n.is_finite() && *n >= 0.0)
                .unwrap_or(duration);
            VideoTimeRange::normalize(duration, start, end, fps)?
        }
    };
    range.ensure_within_per_call_limit()?;
    Ok(range)
}

/// Parse 1-based image index range for directory refs. Default: images 1–200 when unspecified.
pub fn parse_image_dir_range(args: &Value, total_images: usize) -> Result<crate::media::ImageDirRange> {
    use crate::media::image_dir::{ImageDirRange, DEFAULT_IMAGE_BATCH};

    let start = args
        .get("imageStart")
        .and_then(|v| v.as_u64())
        .map(|n| usize::try_from(n).unwrap_or(0));
    let end = args
        .get("imageEnd")
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

    #[test]
    fn parse_pdf_page_range_defaults_to_first_ten() {
        let args = json!({"ref": "x", "mode": "pdf", "goal": "summarize"});
        let range = parse_pdf_page_range(&args, 100).unwrap();
        assert_eq!(range.start, 1);
        assert_eq!(range.end, 10);
        assert!(!range.user_specified);
    }

    #[test]
    fn parse_pdf_page_range_user_range() {
        let args = json!({
            "ref": "x",
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
            "ref": "x",
            "mode": "pdf",
            "goal": "read",
            "pageStart": 1,
            "pageEnd": 20
        });
        assert!(parse_pdf_page_range(&args, 100).is_err());
    }

    #[test]
    fn parse_video_time_range_defaults_first_segment_at_one_fps() {
        let args = json!({"ref": "x", "mode": "video", "goal": "watch"});
        let range = parse_video_time_range(&args, 500.0).unwrap();
        assert_eq!(range.start_sec, 0.0);
        assert_eq!(range.end_sec, 200.0);
        assert_eq!(range.frames_per_second, 1.0);
        assert!(!range.user_specified_time);
        assert_eq!(range.frame_count(), 200);
    }

    #[test]
    fn parse_video_time_range_short_video_uses_full_duration() {
        let args = json!({"ref": "x", "mode": "video", "goal": "watch"});
        let range = parse_video_time_range(&args, 30.0).unwrap();
        assert_eq!(range.end_sec, 30.0);
        assert_eq!(range.frame_count(), 30);
    }

    #[test]
    fn parse_video_time_range_user_window() {
        let args = json!({
            "ref": "x",
            "mode": "video",
            "goal": "watch",
            "timeStartSec": 10.0,
            "timeEndSec": 20.0,
            "framesPerSecond": 2.0
        });
        let range = parse_video_time_range(&args, 120.0).unwrap();
        assert_eq!(range.start_sec, 10.0);
        assert_eq!(range.end_sec, 20.0);
        assert_eq!(range.frames_per_second, 2.0);
        assert!(range.user_specified_time);
        assert_eq!(range.frame_count(), 20);
    }

    #[test]
    fn parse_video_time_range_rejects_too_many_frames() {
        let args = json!({
            "ref": "x",
            "mode": "video",
            "goal": "watch",
            "timeStartSec": 0.0,
            "timeEndSec": 300.0,
            "framesPerSecond": 1.0
        });
        assert!(parse_video_time_range(&args, 300.0).is_err());
    }

    #[test]
    fn parse_image_dir_range_defaults_first_batch() {
        let args = json!({"ref": "/photos", "mode": "image", "goal": "describe"});
        let range = parse_image_dir_range(&args, 500).unwrap();
        assert_eq!(range.start, 1);
        assert_eq!(range.end, 200);
        assert!(!range.user_specified);
    }

    #[test]
    fn parse_image_dir_range_rejects_too_many() {
        let args = json!({
            "ref": "/photos",
            "mode": "image",
            "goal": "describe",
            "imageStart": 1,
            "imageEnd": 250
        });
        assert!(parse_image_dir_range(&args, 500).is_err());
    }
}
