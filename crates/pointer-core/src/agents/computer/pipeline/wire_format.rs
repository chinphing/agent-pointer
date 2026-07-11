//! Per-family wire message formats for Position and Verify LLM phases.
//!
//! Shared JPEG/text helpers live here; each [`OperationFamily`] maps to a format variant.
//! Add a new variant when a tool group needs a different user-message layout.

use crate::agents::computer::screen;
use crate::agents::computer::ScreenCaptureResult;
use crate::agents::computer::vision::screen_overlay::{
    SLOT_SCREEN_AFTER_ACTION, SLOT_SCREEN_ANNOTATED_CURRENT, SLOT_SCREEN_BEFORE_ACTION,
    SLOT_SCREEN_CURRENT,
};
use serde_json::{json, Value};

use super::operation::OperationFamily;

pub const TAG_POSITION: &str = "[POSITION_CONTEXT]";
pub const TAG_VERIFY: &str = "[VERIFY_CONTEXT]";

/// Inputs for position-phase wire assembly.
pub struct PositionWireInput<'a> {
    pub family: OperationFamily,
    pub cap: &'a ScreenCaptureResult,
    pub operation_text: &'a str,
    pub bbox_text: Option<&'a str>,
}

/// Inputs for verify-phase wire assembly.
pub struct VerifyWireInput<'a> {
    pub family: OperationFamily,
    pub before: &'a ScreenCaptureResult,
    pub after: &'a ScreenCaptureResult,
    pub operation_text: &'a str,
    pub tool_result: Option<&'a str>,
}

/// Build position-phase wire messages (system prompt is handled separately).
pub fn build_position_wire_messages(input: &PositionWireInput<'_>) -> Vec<Value> {
    let format = PositionWireFormat::for_family(input.family);
    format.build(input)
}

/// Build verify-phase wire messages (system prompt is handled separately).
pub fn build_verify_wire_messages(input: &VerifyWireInput<'_>) -> Vec<Value> {
    let format = VerifyWireFormat::for_family(input.family);
    format.build(input)
}

// ── Position formats ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum PositionWireFormat {
    /// `[Current screen]` + `[Annotated current screen]` + optional overlay bbox list.
    AnnotatedDualScreen,
}

impl PositionWireFormat {
    fn for_family(family: OperationFamily) -> Self {
        match family {
            OperationFamily::PointerClick
            | OperationFamily::PointerHover
            | OperationFamily::Scroll
            | OperationFamily::Drag
            | OperationFamily::Input
            | OperationFamily::ModifiedClick
            | OperationFamily::Captcha => Self::AnnotatedDualScreen,
            _ => Self::AnnotatedDualScreen,
        }
    }

    fn build(self, input: &PositionWireInput<'_>) -> Vec<Value> {
        match self {
            Self::AnnotatedDualScreen => build_position_annotated_dual_screen(input),
        }
    }
}

const POSITION_ANNOTATED_INSTRUCTION: &str = "\
Compare [Current screen] layout with [Annotated current screen] overlay digits.\n\
Run spatial proof in reasoning_content only; then call exactly one submit_position_* tool (structured args).\n\
Leave message content empty.\n";

fn build_position_annotated_dual_screen(input: &PositionWireInput<'_>) -> Vec<Value> {
    let mut user_text = tagged_operation_block(
        TAG_POSITION,
        input.operation_text,
        POSITION_ANNOTATED_INSTRUCTION,
    );
    if let Some(bbox) = input.bbox_text.filter(|s| !s.trim().is_empty()) {
        user_text.push_str("\n\n");
        user_text.push_str(bbox);
    }
    wire_user_multimodal(vec![
        wire_text_part(&user_text),
        wire_text_part(&format!("{SLOT_SCREEN_CURRENT}\n")),
        wire_jpeg_part(&input.cap.raw_marked_jpeg),
        wire_text_part(&format!("{SLOT_SCREEN_ANNOTATED_CURRENT}\n")),
        wire_jpeg_part(&input.cap.annotated_marked_jpeg),
    ])
}

// ── Verify formats ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum VerifyWireFormat {
    /// Before/after marked screenshots (default for visual desktop actions).
    BeforeAfterScreenshots,
    /// Tool reply + before/after screenshots — cross-check UI context with clipboard text.
    ClipboardWithScreenshots,
}

impl VerifyWireFormat {
    fn for_family(family: OperationFamily) -> Self {
        match family {
            OperationFamily::Clipboard => Self::ClipboardWithScreenshots,
            _ => Self::BeforeAfterScreenshots,
        }
    }

    fn build(self, input: &VerifyWireInput<'_>) -> Vec<Value> {
        match self {
            Self::BeforeAfterScreenshots => build_verify_before_after_screenshots(input),
            Self::ClipboardWithScreenshots => build_verify_clipboard_with_screenshots(input),
        }
    }
}

const VERIFY_SCREENSHOT_INSTRUCTION: &str = "\
Judge whether the last operation succeeded using evidence described in the Scenario section.\n\
Both images include synthetic pointer overlay at capture time.\n\
Run judgment in reasoning_content only; then call `submit_verify`.\n\
Leave message content empty.\n";

const VERIFY_CLIPBOARD_INSTRUCTION: &str = "\
Judge whether the clipboard content in [Tool result] matches the expected goal.\n\
Use [Screen before action] and [Screen after action] as supporting context — e.g. visible\n\
copied field, toast, selection — but treat clipboard bytes as authoritative from [Tool result].\n\
Fail when tool-result text contradicts the goal even if the UI looks plausible.\n\
Run judgment in reasoning_content only; then call `submit_verify`.\n\
Leave message content empty.\n";

fn build_verify_before_after_screenshots(input: &VerifyWireInput<'_>) -> Vec<Value> {
    let before_jpeg = input.before.raw_marked_jpeg.as_slice();
    log::info!(
        "pipeline vision: verify slots before=T0 raw_marked_jpeg ({} bytes, label {}) \
         after=fresh capture ({} bytes, label {})",
        before_jpeg.len(),
        SLOT_SCREEN_BEFORE_ACTION,
        input.after.raw_marked_jpeg.len(),
        SLOT_SCREEN_AFTER_ACTION,
    );
    let user_text = tagged_operation_block(
        TAG_VERIFY,
        input.operation_text,
        VERIFY_SCREENSHOT_INSTRUCTION,
    );
    wire_user_multimodal(vec![
        wire_text_part(&user_text),
        wire_text_part(&format!("{SLOT_SCREEN_BEFORE_ACTION}\n")),
        wire_jpeg_part(before_jpeg),
        wire_text_part(&format!("{SLOT_SCREEN_AFTER_ACTION}\n")),
        wire_jpeg_part(&input.after.raw_marked_jpeg),
    ])
}

fn build_verify_clipboard_with_screenshots(input: &VerifyWireInput<'_>) -> Vec<Value> {
    let before_jpeg = input.before.raw_marked_jpeg.as_slice();
    let tool_block = input
        .tool_result
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("(tool result not available)");
    log::info!(
        "pipeline vision: verify clipboard slots before=T0 raw_marked_jpeg ({} bytes) \
         after=fresh capture ({} bytes) tool_result_chars={}",
        before_jpeg.len(),
        input.after.raw_marked_jpeg.len(),
        tool_block.len(),
    );
    let user_text = format!(
        "{}\n\n[Tool result]\n{tool_block}\n\n{VERIFY_CLIPBOARD_INSTRUCTION}",
        tagged_operation_block(TAG_VERIFY, input.operation_text, ""),
        tool_block = tool_block,
    );
    wire_user_multimodal(vec![
        wire_text_part(&user_text),
        wire_text_part(&format!("{SLOT_SCREEN_BEFORE_ACTION}\n")),
        wire_jpeg_part(before_jpeg),
        wire_text_part(&format!("{SLOT_SCREEN_AFTER_ACTION}\n")),
        wire_jpeg_part(&input.after.raw_marked_jpeg),
    ])
}

// ── Shared wire helpers ──────────────────────────────────────────────────────

fn tagged_operation_block(tag: &str, operation_text: &str, instruction: &str) -> String {
    let op = operation_text.trim();
    let instr = instruction.trim();
    if instr.is_empty() {
        format!("{tag}\n{op}\n")
    } else {
        format!("{tag}\n{op}\n\n{instr}")
    }
}

fn wire_text_part(text: &str) -> Value {
    json!({ "type": "text", "text": text })
}

fn wire_jpeg_part(jpeg: &[u8]) -> Value {
    json!({
        "type": "image_url",
        "image_url": {
            "url": format!("data:image/jpeg;base64,{}", screen::encode_image_to_base64(jpeg))
        }
    })
}

fn wire_user_multimodal(content: Vec<Value>) -> Vec<Value> {
    vec![json!({
        "role": "user",
        "content": content,
    })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::computer::vision::screen::MonitorInfo;

    fn empty_cap() -> ScreenCaptureResult {
        ScreenCaptureResult {
            raw_unmarked_jpeg: Vec::new(),
            raw_marked_jpeg: vec![1, 2, 3],
            annotated_marked_jpeg: vec![4, 5, 6],
            zoom_menu_bar_png: Vec::new(),
            zoom_task_bar_png: Vec::new(),
            zoom_pointer_png: Vec::new(),
            mouse_neighbor_reference_text: None,
            monitor: MonitorInfo::new(0, 0, 1920, 1080),
            inject_before_action: None,
        }
    }

    #[test]
    fn position_default_format_includes_dual_images() {
        let cap = empty_cap();
        let wire = build_position_wire_messages(&PositionWireInput {
            family: OperationFamily::PointerClick,
            cap: &cap,
            operation_text: r#"tool=click goal="Save" action="Click Save""#,
            bbox_text: Some("Overlay reference bboxes: R=1 ..."),
        });
        let content = wire[0].get("content").unwrap().as_array().unwrap();
        assert_eq!(content.len(), 5);
        assert!(content[0].get("text").unwrap().as_str().unwrap().contains(TAG_POSITION));
        assert!(content[0].get("text").unwrap().as_str().unwrap().contains("Overlay reference"));
    }

    #[test]
    fn verify_clipboard_includes_tool_result_and_screenshots() {
        let cap = empty_cap();
        let wire = build_verify_wire_messages(&VerifyWireInput {
            family: OperationFamily::Clipboard,
            before: &cap,
            after: &cap,
            operation_text: r#"tool=clipboard_read goal="Read API key" action="""#,
            tool_result: Some("Goal: Read API key. Clipboard text:\n\nsk-test123"),
        });
        let content = wire[0].get("content").unwrap().as_array().unwrap();
        assert_eq!(content.len(), 5);
        let text = content[0].get("text").unwrap().as_str().unwrap();
        assert!(text.contains("[Tool result]"));
        assert!(text.contains("sk-test123"));
        assert!(text.contains("clipboard bytes as authoritative"));
        assert!(content[1].get("text").unwrap().as_str().unwrap().contains(SLOT_SCREEN_BEFORE_ACTION));
        assert!(content[3].get("text").unwrap().as_str().unwrap().contains(SLOT_SCREEN_AFTER_ACTION));
    }

    #[test]
    fn verify_pointer_click_uses_before_after_images() {
        let cap = empty_cap();
        let wire = build_verify_wire_messages(&VerifyWireInput {
            family: OperationFamily::PointerClick,
            before: &cap,
            after: &cap,
            operation_text: r#"tool=mouse_click_index goal="Save" action="""#,
            tool_result: None,
        });
        let content = wire[0].get("content").unwrap().as_array().unwrap();
        assert_eq!(content.len(), 5);
        assert!(content[0].get("text").unwrap().as_str().unwrap().contains("before vs after"));
    }
}
