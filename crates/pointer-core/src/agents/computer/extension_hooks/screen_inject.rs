//! `message_loop_prompts_after` — `_10_computer_screen_inject` (Python `agents/computer/extensions/...` analogue).
//!
//! OS / locale / **calendar date** for the model live in the **last slice** of merged **`system`**
//! text (`chat_service::prompts::push_env_to_cacheable`, before `before_main_llm_call`).
//! **Full date and time at capture** is prefixed on this hook’s `[CUR_SCREEN]` **`user`** message
//! (`crate::env_prompt::format_local_wall_clock_full`).

use crate::agents::computer::capture_debug;
use crate::agents::computer::tool_names::ACTION_VERIFY;
use crate::agents::computer::screen;
use crate::agents::computer::screen_overlay::{
    BEFORE_POINTER_ZOOM_CROP_SIDE, BEFORE_POINTER_ZOOM_FACTOR, BEFORE_POINTER_ZOOM_RADIUS_PX,
    SLOT_SCREEN_AFTER_ACTION, SLOT_SCREEN_ANNOTATED, SLOT_SCREEN_BEFORE_ACTION,
    SLOT_SCREEN_ZOOMED_BOTTOM, SLOT_SCREEN_ZOOMED_POINTER,
    SLOT_SCREEN_ZOOMED_POINTER_BEFORE, SLOT_SCREEN_ZOOMED_TOP,
};
use crate::agents::computer::tier::ComputerTier;
use crate::agents::computer::ScreenCaptureResult;
use crate::agents::AgentProfile;
use crate::extensions::{
    new_extension_message_id, ExtensionRegistry, MessageLoopPromptsAfterContext,
    MessageLoopPromptsAfterHook,
};
use crate::models::{ChatMessage, Role, StreamEvent};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

const CUR_SCREEN_TAG: &str = "[CUR_SCREEN]";

/// Thread notice while capture + processing runs (user-visible).
const DESKTOP_NOTICE_PROCESSING: &str = "【桌面】截图处理中";
const DESKTOP_NOTICE_READY: &str = "【桌面】已更新当前画面。";
/// User-facing failure copy — do not echo internal/annotation-service errors.
const DESKTOP_NOTICE_FAILED: &str =
    "【桌面】截图处理失败，请检查屏幕录制权限或截图处理服务是否可用后重试。";

fn cur_screen_failure_model_message() -> String {
    format!(
        "{tag} Screenshot processing failed this turn.\n\
         You cannot rely on a fresh desktop image. \
         The user may need screen capture permission or the screenshot processing service; suggest retrying after that.",
        tag = CUR_SCREEN_TAG
    )
}

fn cur_screen_clock_prefix() -> String {
    format!(
        "Local wall-clock at capture: {}\n\n",
        crate::env_prompt::format_local_wall_clock_full()
    )
}

/// When stripping prior vision, replace stale `[CUR_SCREEN]` prose so the model is not told about screenshots that are no longer attached.
const CUR_SCREEN_HISTORY_PLACEHOLDER: &str = "[CUR_SCREEN] Earlier desktop screenshots are omitted here; use only the latest [CUR_SCREEN] message in this request for images.\n";

/// Remove vision payloads from all messages already in history so older frames do not affect the model’s read of the latest `[CUR_SCREEN]`.
pub(crate) fn strip_images_from_prior_messages(messages: &mut [ChatMessage]) {
    for m in messages.iter_mut() {
        m.images_base64 = None;
        m.image_slot_labels = None;
        if matches!(m.role, Role::User) && m.content.trim_start().starts_with(CUR_SCREEN_TAG) {
            m.content = CUR_SCREEN_HISTORY_PLACEHOLDER.to_string();
        }
    }
}

/// Labels in wire order — must match `assemble_cur_screen_base64` image sequence.
fn slot_labels_for_tier(tier: ComputerTier, has_previous_raw: bool) -> Vec<&'static str> {
    match tier {
        ComputerTier::Primary => {
            let mut labels = Vec::with_capacity(3);
            if has_previous_raw {
                labels.push(SLOT_SCREEN_BEFORE_ACTION);
            }
            labels.push(SLOT_SCREEN_AFTER_ACTION);
            labels.push(SLOT_SCREEN_ANNOTATED);
            labels
        }
        ComputerTier::Intermediate => {
            let mut labels = Vec::with_capacity(3);
            if has_previous_raw {
                labels.push(SLOT_SCREEN_BEFORE_ACTION);
            }
            labels.push(SLOT_SCREEN_AFTER_ACTION);
            labels.push(SLOT_SCREEN_ANNOTATED);
            labels
        }
        ComputerTier::Advanced => {
            let mut labels = Vec::with_capacity(7);
            if has_previous_raw {
                labels.push(SLOT_SCREEN_BEFORE_ACTION);
                labels.push(SLOT_SCREEN_ZOOMED_POINTER_BEFORE);
            }
            labels.push(SLOT_SCREEN_AFTER_ACTION);
            labels.push(SLOT_SCREEN_ANNOTATED);
            labels.push(SLOT_SCREEN_ZOOMED_TOP);
            labels.push(SLOT_SCREEN_ZOOMED_BOTTOM);
            labels.push(SLOT_SCREEN_ZOOMED_POINTER);
            labels
        }
    }
}

fn build_cur_screen_preamble(tier: ComputerTier, has_previous_raw: bool) -> String {
    let cite = "Each screenshot below is preceded by its slot label on its own line. Treat only what you see in that labeled image as ground truth — when reasoning internally, cite **On [slot name]:**; do not invent UI from task text or prior turns. Do not write internal checklists in assistant message text. When the user must see a reply (question, blockage, completion), write plain text in **content** in the same turn — reasoning alone is invisible to the user.";
    match tier {
        ComputerTier::Primary => format!(
            "{CUR_SCREEN_TAG} Primary uses two or three labeled images this turn: optional {SLOT_SCREEN_BEFORE_ACTION}, then {SLOT_SCREEN_AFTER_ACTION}, then {SLOT_SCREEN_ANNOTATED}. {cite} \
             Text below includes **Pointer position** and **Nearby overlay reference bboxes** (10 nearest the pointer; session 0–1000 rects). \
             **Verify:** compare before/after first; if first capture, before is n/a. \
             **Next:** judge **N–target relation** (inner-center-wrap / inner-edge-wrap / unwrapped), then choose index or coordinate route.\n"
        ),
        ComputerTier::Intermediate => format!(
            "{CUR_SCREEN_TAG} Primary uses two or three labeled images this turn: optional {SLOT_SCREEN_BEFORE_ACTION}, then {SLOT_SCREEN_AFTER_ACTION}, then {SLOT_SCREEN_ANNOTATED}. {cite} \
             Text below includes **Pointer position** and **Nearby overlay reference bboxes** (10 nearest the pointer; session 0–1000 rects). \
             **Verify:** compare before/after first; if first capture, before is n/a. \
             **Next:** judge **N–target relation** (inner-center-wrap / inner-edge-wrap / unwrapped), then choose index or coordinate route.\n"
        ),
        ComputerTier::Advanced => {
            let zoom_before = if has_previous_raw {
                format!(
                    " **{SLOT_SCREEN_ZOOMED_POINTER_BEFORE}** is a **{factor}×** magnified **{crop}×{crop} px** crop (±{radius} px around the pointer) from **{SLOT_SCREEN_BEFORE_ACTION}** — use it for **Pointer:** hotspot-vs-center geometry.",
                    factor = BEFORE_POINTER_ZOOM_FACTOR,
                    crop = BEFORE_POINTER_ZOOM_CROP_SIDE,
                    radius = BEFORE_POINTER_ZOOM_RADIUS_PX,
                )
            } else {
                String::new()
            };
            let count = if has_previous_raw { 7 } else { 5 };
            format!(
                "{CUR_SCREEN_TAG} {count} labeled images follow in slot order.{zoom_before} {cite} \
                 Run Verify (screenshots) first; Pointer only if unclear; then Repetition, Next, Location, Recheck, Tool route — all internally; report via `{ACTION_VERIFY}`.\n"
            )
        }
    }
}

fn assemble_cur_screen_base64(tier: ComputerTier, cap: &ScreenCaptureResult) -> Vec<String> {
    match tier {
        ComputerTier::Primary => {
            let mut out = Vec::with_capacity(3);
            if let Some(before) = &cap.inject_before_action {
                out.push(screen::encode_image_to_base64(&before.screen_jpeg));
            }
            out.push(screen::encode_image_to_base64(&cap.raw_marked_jpeg));
            out.push(screen::encode_image_to_base64(&cap.annotated_marked_jpeg));
            out
        }
        ComputerTier::Intermediate => {
            let mut out = Vec::with_capacity(3);
            if let Some(before) = &cap.inject_before_action {
                out.push(screen::encode_image_to_base64(&before.screen_jpeg));
            }
            out.push(screen::encode_image_to_base64(&cap.raw_marked_jpeg));
            out.push(screen::encode_image_to_base64(&cap.annotated_marked_jpeg));
            out
        }
        ComputerTier::Advanced => assemble_cur_screen_base64_advanced(cap),
    }
}

fn assemble_cur_screen_base64_advanced(cap: &ScreenCaptureResult) -> Vec<String> {
    let mut out = Vec::with_capacity(7);
    if let Some(before) = &cap.inject_before_action {
        out.push(screen::encode_image_to_base64(&before.screen_jpeg));
        out.push(screen::encode_image_to_base64(&before.zoom_pointer_png));
    }
    out.push(screen::encode_image_to_base64(&cap.raw_marked_jpeg));
    out.push(screen::encode_image_to_base64(&cap.annotated_marked_jpeg));
    out.push(screen::encode_image_to_base64(&cap.zoom_menu_bar_png));
    out.push(screen::encode_image_to_base64(&cap.zoom_task_bar_png));
    out.push(screen::encode_image_to_base64(&cap.zoom_pointer_png));
    out
}

fn assemble_cur_screen_payload(
    tier: ComputerTier,
    cap: &ScreenCaptureResult,
) -> (Vec<String>, Vec<String>) {
    let has_previous_raw = cap.inject_before_action.is_some();
    let labels: Vec<String> = slot_labels_for_tier(tier, has_previous_raw)
        .into_iter()
        .map(str::to_string)
        .collect();
    let images = assemble_cur_screen_base64(tier, cap);
    debug_assert_eq!(
        labels.len(),
        images.len(),
        "slot labels must match image count"
    );
    (labels, images)
}

pub fn register(registry: &mut ExtensionRegistry) {
    registry.register_message_loop_prompts_after(Arc::new(ComputerScreenInject));
}

fn emit_screen_thread_notice(ctx: &MessageLoopPromptsAfterContext<'_>, message_id: String, content: String) {
    let Some(tx) = ctx.stream else {
        return;
    };
    let _ = tx.send(StreamEvent::InjectedAssistantMessage {
        conversation_id: ctx.conversation_id.to_string(),
        message_id,
        content,
    });
}

fn emit_screen_notice_update(ctx: &MessageLoopPromptsAfterContext<'_>, message_id: String, content: String) {
    let Some(tx) = ctx.stream else {
        return;
    };
    let _ = tx.send(StreamEvent::InjectedAssistantMessageUpdate {
        conversation_id: ctx.conversation_id.to_string(),
        message_id,
        content,
    });
}

struct ComputerScreenInject;

#[async_trait]
impl MessageLoopPromptsAfterHook for ComputerScreenInject {
    fn override_key(&self) -> &'static str {
        "_10_computer_screen_inject"
    }

    fn sort_key(&self) -> &'static str {
        "_10_computer_screen_inject"
    }

    async fn execute(&self, ctx: &mut MessageLoopPromptsAfterContext<'_>) -> Result<()> {
        if ctx.lead_agent_profile != AgentProfile::Computer {
            return Ok(());
        }

        let notice_id = new_extension_message_id("screen_notice");
        emit_screen_thread_notice(
            ctx,
            notice_id.clone(),
            DESKTOP_NOTICE_PROCESSING.to_string(),
        );

        match ctx.computer_state.capture_and_annotate(ctx.conversation_id).await {
            Ok((cap, refreshed_monitor_id)) => {
                if let (Some(tx), Some(new_id)) = (ctx.stream, refreshed_monitor_id.as_ref()) {
                    let _ = tx.send(StreamEvent::ComputerMonitorUpdated {
                        conversation_id: ctx.conversation_id.to_string(),
                        monitor_id: Some(new_id.clone()),
                    });
                }
                let dump_prefix = ctx
                    .round_screen_dump_prefix
                    .as_deref()
                    .or(ctx.round_assistant_message_id.as_deref())
                    .unwrap_or("round_unknown");
                let tier = ctx.computer_state.tier_for_conversation(ctx.conversation_id);
                let annotated_rel = capture_debug::save_computer_capture_debug(
                    ctx.conversation_id,
                    dump_prefix,
                    &cap,
                    tier,
                );
                if let (Some(tx), Some(mid)) = (ctx.stream, ctx.round_assistant_message_id.as_ref()) {
                    if let Some(rel) = annotated_rel {
                        let _ = tx.send(StreamEvent::AssistantRoundScreen {
                            conversation_id: ctx.conversation_id.to_string(),
                            message_id: mid.clone(),
                            annotated_rel_path: rel,
                        });
                    }
                }
                emit_screen_notice_update(
                    ctx,
                    notice_id,
                    DESKTOP_NOTICE_READY.to_string(),
                );
                strip_images_from_prior_messages(ctx.messages.as_mut_slice());
                let has_previous_raw = cap.inject_before_action.is_some();
                let (image_slot_labels, images) = assemble_cur_screen_payload(tier, &cap);
                let mut text = cur_screen_clock_prefix();
                if let Some(label) = ctx
                    .computer_state
                    .locked_goal_label(ctx.conversation_id)
                {
                    text.push_str(&format!("Locked goal: {label}\n\n"));
                }
                text.push_str(&build_cur_screen_preamble(tier, has_previous_raw));
                if let Some(block) = ctx.computer_state.recent_actions_prompt_block(ctx.conversation_id) {
                    text.push_str("\n\n");
                    text.push_str(&block);
                    text.push('\n');
                }
                if let Some(ref anchor) = cap.mouse_neighbor_reference_text {
                    text.push_str("\n\n");
                    text.push_str(anchor);
                }
                ctx.messages.push(ChatMessage {
                    id: new_extension_message_id("screen_inject"),
                    role: Role::User,
                    content: text,
                    status: "done".into(),
                    created_at: crate::extensions::now_ms(),
                    tool_calls: None,
                    tool_call_id: None,
                    error_message: None,
                    reasoning: None,
                    thoughts: None,
                    headline: None,
                    raw_content: None,
                    tool_raw_output: None,
                    agent_id: None,
                    agent_instance_id: None,
                    agent_name: None,
                    agent_trace: None,
                    image_slot_labels: Some(image_slot_labels),
                    images_base64: Some(images),
                    computer_round_screen_rel_path: None,
        ui_bindings: None,
            context_state: None,
        attachments: None,
            });
            }
            Err(e) => {
                log::warn!("computer screenshot processing failed: {:#}", e);
                emit_screen_notice_update(ctx, notice_id, DESKTOP_NOTICE_FAILED.to_string());
                strip_images_from_prior_messages(ctx.messages.as_mut_slice());
                ctx.messages.push(ChatMessage {
                    id: new_extension_message_id("screen_inject"),
                    role: Role::User,
                    content: format!(
                        "{}{}",
                        cur_screen_clock_prefix(),
                        cur_screen_failure_model_message()
                    ),
                    status: "done".into(),
                    created_at: crate::extensions::now_ms(),
                    tool_calls: None,
                    tool_call_id: None,
                    error_message: None,
                    reasoning: None,
                    thoughts: None,
                    headline: None,
                    raw_content: None,
                    tool_raw_output: None,
                    agent_id: None,
                    agent_instance_id: None,
                    agent_name: None,
                    agent_trace: None,
                    image_slot_labels: None,
                    images_base64: None,
                    computer_round_screen_rel_path: None,
        ui_bindings: None,
            context_state: None,
        attachments: None,
            });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::computer::ScreenCaptureResult;
    use crate::models::ChatMessage;

    fn msg_with_images(images: Option<Vec<&str>>) -> ChatMessage {
        ChatMessage {
            id: "m1".into(),
            role: Role::User,
            content: "[CUR_SCREEN] test".into(),
            status: "done".into(),
            created_at: 0,
            tool_calls: None,
            tool_call_id: None,
            error_message: None,
            reasoning: None,
            thoughts: None,
            headline: None,
            raw_content: None,
            tool_raw_output: None,
            agent_id: None,
            agent_instance_id: None,
            agent_name: None,
            agent_trace: None,
            image_slot_labels: None,
            images_base64: images.map(|v| v.into_iter().map(String::from).collect()),
            computer_round_screen_rel_path: None,
        ui_bindings: None,
            context_state: None,
        attachments: None,
            }
    }

    fn dummy_cap(has_before: bool) -> ScreenCaptureResult {
        use crate::agents::computer::screen::MonitorInfo;
        use crate::agents::computer::screen_overlay::BeforeActionInject;
        ScreenCaptureResult {
            raw_unmarked_jpeg: vec![1, 2],
            raw_marked_jpeg: vec![3, 4],
            annotated_marked_jpeg: vec![5, 6],
            zoom_menu_bar_png: vec![7],
            zoom_task_bar_png: vec![8],
            zoom_pointer_png: vec![9],
            mouse_neighbor_reference_text: None,
            monitor: MonitorInfo::new(0, 0, 100, 100),
            inject_before_action: has_before.then(|| BeforeActionInject {
                screen_jpeg: vec![10],
                zoom_pointer_png: vec![11],
            }),
        }
    }

    #[test]
    fn strip_prior_clears_images_without_touching_non_cur_screen_text() {
        let mut msgs = vec![ChatMessage {
            content: "Plain user text.".into(),
            ..msg_with_images(Some(vec!["aaa"]))
        }];
        strip_images_from_prior_messages(&mut msgs);
        assert!(msgs[0].images_base64.is_none());
        assert!(msgs[0].image_slot_labels.is_none());
        assert_eq!(msgs[0].content, "Plain user text.");
    }

    #[test]
    fn strip_prior_replaces_cur_screen_text_when_images_removed() {
        let mut msgs = vec![msg_with_images(Some(vec!["aaa"]))];
        strip_images_from_prior_messages(&mut msgs);
        assert!(msgs[0].images_base64.is_none());
        assert_eq!(msgs[0].content, CUR_SCREEN_HISTORY_PLACEHOLDER);
    }

    #[test]
    fn strip_prior_clears_all_messages() {
        let mut msgs = vec![
            msg_with_images(Some(vec!["a"])),
            ChatMessage {
                content: "hello".into(),
                ..msg_with_images(Some(vec!["b"]))
            },
        ];
        strip_images_from_prior_messages(&mut msgs);
        assert!(msgs[0].images_base64.is_none());
        assert!(msgs[1].images_base64.is_none());
        assert_eq!(msgs[0].content, CUR_SCREEN_HISTORY_PLACEHOLDER);
        assert_eq!(msgs[1].content, "hello");
    }

    #[test]
    fn slot_labels_match_image_count_primary_with_optional_before() {
        let cap_no_before = dummy_cap(false);
        let (labels_no_before, images_no_before) =
            assemble_cur_screen_payload(ComputerTier::Primary, &cap_no_before);
        assert_eq!(labels_no_before.len(), 2);
        assert_eq!(labels_no_before, vec![SLOT_SCREEN_AFTER_ACTION, SLOT_SCREEN_ANNOTATED]);
        assert_eq!(labels_no_before.len(), images_no_before.len());

        let cap_with_before = dummy_cap(true);
        let (labels_with_before, images_with_before) =
            assemble_cur_screen_payload(ComputerTier::Primary, &cap_with_before);
        assert_eq!(labels_with_before.len(), 3);
        assert_eq!(
            labels_with_before,
            vec![SLOT_SCREEN_BEFORE_ACTION, SLOT_SCREEN_AFTER_ACTION, SLOT_SCREEN_ANNOTATED]
        );
        assert_eq!(labels_with_before.len(), images_with_before.len());
    }

    #[test]
    fn slot_labels_match_image_count_advanced_without_before() {
        let cap = dummy_cap(false);
        let (labels, images) =
            assemble_cur_screen_payload(ComputerTier::Advanced, &cap);
        assert_eq!(labels.len(), 5);
        assert_eq!(labels.len(), images.len());
        assert_eq!(labels[0], SLOT_SCREEN_AFTER_ACTION);
        assert_eq!(labels[4], SLOT_SCREEN_ZOOMED_POINTER);
    }

    #[test]
    fn slot_labels_match_image_count_advanced_with_before() {
        let cap = dummy_cap(true);
        let (labels, images) =
            assemble_cur_screen_payload(ComputerTier::Advanced, &cap);
        assert_eq!(labels.len(), 7);
        assert_eq!(labels.len(), images.len());
        assert!(labels.contains(&SLOT_SCREEN_BEFORE_ACTION.to_string()));
        assert!(labels.contains(&SLOT_SCREEN_ZOOMED_POINTER_BEFORE.to_string()));
    }

    #[test]
    fn preamble_mentions_labeled_images() {
        let t = build_cur_screen_preamble(ComputerTier::Advanced, false);
        assert!(t.contains("labeled images"));
        assert!(t.contains("On [slot name]:"));
    }
}
