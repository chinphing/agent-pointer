//! `message_loop_prompts_after` — `_10_computer_screen_inject` (Python `agents/computer/extensions/...` analogue).
//!
//! OS / locale / **calendar date** for the model live in the **last slice** of merged **`system`**
//! text (`chat_service::prompts::push_env_and_json_wire_tail_to_cacheable`, before `before_main_llm_call`).
//! **Full date and time at capture** is prefixed on this hook’s `[CUR_SCREEN]` **`user`** message
//! (`crate::env_prompt::format_local_wall_clock_full`).

use crate::agents::computer::capture_debug;
use crate::agents::computer::screen;
use crate::agents::computer::screen_overlay::{
    BEFORE_POINTER_ZOOM_CROP_SIDE, BEFORE_POINTER_ZOOM_FACTOR, BEFORE_POINTER_ZOOM_RADIUS_PX,
    SLOT_SCREEN_AFTER_ACTION, SLOT_SCREEN_ANNOTATED, SLOT_SCREEN_BEFORE_ACTION,
    SLOT_SCREEN_ZOOMED_BOTTOM, SLOT_SCREEN_ZOOMED_POINTER, SLOT_SCREEN_ZOOMED_POINTER_BEFORE,
    SLOT_SCREEN_ZOOMED_TOP,
};
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

fn cur_screen_clock_prefix() -> String {
    format!(
        "Local wall-clock at capture: {}\n\n",
        crate::env_prompt::format_local_wall_clock_full()
    )
}

/// When stripping prior vision, replace stale `[CUR_SCREEN]` prose (frame order / labels) so the model is not told about screenshots that are no longer attached.
const CUR_SCREEN_HISTORY_PLACEHOLDER: &str = "[CUR_SCREEN] Earlier desktop screenshots are omitted here; use only the latest [CUR_SCREEN] message in this request for images.\n";

/// Remove vision payloads from all messages already in history so older frames do not affect the model’s read of the latest `[CUR_SCREEN]`.
///
/// Historical `[CUR_SCREEN]` user turns would otherwise keep long text listing `[Screen before action]`, etc., with **no** `image_url` parts after this — that mismatch can confuse the model. Those messages get a short placeholder instead.
pub(crate) fn strip_images_from_prior_messages(messages: &mut [ChatMessage]) {
    for m in messages.iter_mut() {
        m.images_base64 = None;
        if matches!(m.role, Role::User) && m.content.trim_start().starts_with(CUR_SCREEN_TAG) {
            m.content = CUR_SCREEN_HISTORY_PLACEHOLDER.to_string();
        }
    }
}

fn build_cur_screen_text(has_previous_raw: bool) -> String {
    let zoom_before = if has_previous_raw {
        format!(
            " {SLOT_SCREEN_ZOOMED_POINTER_BEFORE} is a **{factor}×** magnified **{crop}×{crop} px** crop (±{radius} px radius around the pointer) from **[Screen before action]** — use it as the **standard** for **Pointer:** hotspot-vs-center geometry.",
            factor = BEFORE_POINTER_ZOOM_FACTOR,
            crop = BEFORE_POINTER_ZOOM_CROP_SIDE,
            radius = BEFORE_POINTER_ZOOM_RADIUS_PX,
        )
    } else {
        String::new()
    };
    let before_line = if has_previous_raw {
        "[Screen before action] is the **previous** turn’s unmarked full-screen capture with the **current** synthetic pointer — desktop layout **before** the last automated step."
    } else {
        ""
    };
    let tail = format!(
        "{before_line}{zoom_before} Slot names label each image. Every visual claim in thoughts must cite On [slot name]:. Stage rules and Location routing: runtime COMMUNICATION.md."
    );
    let hint = if has_previous_raw {
        format!(
            "Compare [Screen before action] to [Screen after action] for task-relevant UI change; judge pointer hotspot vs intended center on {SLOT_SCREEN_ZOOMED_POINTER_BEFORE} when present; {tail}"
        )
    } else {
        format!("[Screen after action] is the current full-screen capture; {tail}")
    };
    let order = if has_previous_raw {
        format!(
            "Order: (1) {SLOT_SCREEN_BEFORE_ACTION} (2) {SLOT_SCREEN_ZOOMED_POINTER_BEFORE} (3) {SLOT_SCREEN_AFTER_ACTION} (4) {SLOT_SCREEN_ANNOTATED} (5) {SLOT_SCREEN_ZOOMED_TOP} (6) {SLOT_SCREEN_ZOOMED_BOTTOM} (7) {SLOT_SCREEN_ZOOMED_POINTER}. {hint}"
        )
    } else {
        format!(
            "Order: (1) {SLOT_SCREEN_AFTER_ACTION} (2) {SLOT_SCREEN_ANNOTATED} (3) {SLOT_SCREEN_ZOOMED_TOP} (4) {SLOT_SCREEN_ZOOMED_BOTTOM} (5) {SLOT_SCREEN_ZOOMED_POINTER}. {hint}"
        )
    };
    format!("{CUR_SCREEN_TAG} {order}\n")
}

fn assemble_cur_screen_base64(cap: &ScreenCaptureResult) -> Vec<String> {
    let mut out = Vec::with_capacity(7);
    if let Some(before) = &cap.inject_before_action {
        out.push(screen::encode_image_to_base64(&before.screen_jpeg));
        out.push(screen::encode_image_to_base64(&before.zoom_pointer_png));
    }
    out.push(screen::encode_image_to_base64(&cap.raw_marked_jpeg));
    out.push(screen::encode_image_to_base64(&cap.annotated_marked_png));
    out.push(screen::encode_image_to_base64(&cap.zoom_menu_bar_png));
    out.push(screen::encode_image_to_base64(&cap.zoom_task_bar_png));
    out.push(screen::encode_image_to_base64(&cap.zoom_pointer_png));
    out
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
            "【桌面】正在截图并标注…".to_string(),
        );

        match ctx.computer_state.capture_and_annotate(ctx.conversation_id).await {
            Ok(cap) => {
                let dump_prefix = ctx
                    .round_screen_dump_prefix
                    .as_deref()
                    .or(ctx.round_assistant_message_id.as_deref())
                    .unwrap_or("round_unknown");
                let annotated_rel =
                    capture_debug::save_computer_capture_debug(ctx.conversation_id, dump_prefix, &cap);
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
                    "【桌面】已更新当前画面（原图/标注/放大）。".to_string(),
                );
                strip_images_from_prior_messages(ctx.messages.as_mut_slice());
                let has_previous_raw = cap.inject_before_action.is_some();
                let images = assemble_cur_screen_base64(&cap);
                let mut text = cur_screen_clock_prefix();
                text.push_str(&build_cur_screen_text(has_previous_raw));
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
                    agent_id: None,
                    agent_name: None,
                    agent_trace: None,
                    images_base64: Some(images),
                    computer_round_screen_rel_path: None,
                });
            }
            Err(e) => {
                log::warn!("computer screen capture/annotate failed: {:#}", e);
                emit_screen_notice_update(
                    ctx,
                    notice_id,
                    format!("【桌面】截图或标注失败：{e}"),
                );
                strip_images_from_prior_messages(ctx.messages.as_mut_slice());
                ctx.messages.push(ChatMessage {
                    id: new_extension_message_id("screen_inject"),
                    role: Role::User,
                    content: format!(
                        "{}{CUR_SCREEN_TAG} Screen capture or UI annotation failed: {e}\nYou cannot rely on a fresh desktop image this turn. The user may need to grant screen capture access or ensure the desktop annotation service is available; suggest retrying after that.",
                        cur_screen_clock_prefix()
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
                    agent_id: None,
                    agent_name: None,
                    agent_trace: None,
                    images_base64: None,
                    computer_round_screen_rel_path: None,
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            agent_id: None,
            agent_name: None,
            agent_trace: None,
            images_base64: images.map(|v| v.into_iter().map(String::from).collect()),
            computer_round_screen_rel_path: None,
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
    fn legend_lists_all_slots_when_no_prev() {
        let t = build_cur_screen_text(false);
        assert!(t.contains(SLOT_SCREEN_AFTER_ACTION));
        assert!(t.contains(SLOT_SCREEN_ZOOMED_POINTER));
        assert!(!t.contains(&format!("(2) {SLOT_SCREEN_ZOOMED_POINTER_BEFORE}")));
    }

    #[test]
    fn legend_lists_before_zoom_when_prev() {
        let t = build_cur_screen_text(true);
        assert!(t.contains(SLOT_SCREEN_ZOOMED_POINTER_BEFORE));
        assert!(t.contains(SLOT_SCREEN_BEFORE_ACTION));
    }
}
