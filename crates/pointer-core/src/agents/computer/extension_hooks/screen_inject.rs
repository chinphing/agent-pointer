//! `message_loop_prompts_after` — `_10_computer_screen_inject` (Python `agents/computer/extensions/...` analogue).
//!
//! OS / locale / **calendar date** for the model live in the **last slice** of merged **`system`**
//! text (`chat_service::prompts::push_env_context_last_in_system_prompts`, after `before_main_llm_call` hooks).
//! **Full date and time at capture** is prefixed on this hook’s `[CUR_SCREEN]` **`user`** message
//! (`crate::env_prompt::format_local_wall_clock_full`).

use crate::agents::computer::capture_debug;
use crate::agents::computer::screen;
use crate::agents::computer::screen_overlay::{
    SLOT_SCREEN_AFTER_ACTION, SLOT_SCREEN_ANNOTATED, SLOT_SCREEN_BEFORE_ACTION,
    SLOT_SCREEN_ZOOMED_BOTTOM, SLOT_SCREEN_ZOOMED_POINTER, SLOT_SCREEN_ZOOMED_TOP,
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
    let tail = "[Annotated after action] carries overlay index numbers (not shown on [Screen before action]). [Zoom top after action], [Zoom bottom after action], and [Zoom pointer after action] magnify that same after-action view. A pointer and text caret may be drawn on full-screen captures and on the annotated image. When the next block begins with **Pointer position**, it gives the synthetic pointer in **capture pixels** and (for this session’s coordinate tools) **normalized 0–1000** on the full capture, immediately followed by **Pointer neighbor reference bboxes**: up to five nearest overlay regions (centers inside a **300×300 px** window centered on that position) as **coordinate** anchors only, or **None** plus guidance to aim **directly** at the visible target without requiring those anchors.";
    let hint = if has_previous_raw {
        format!(
            "Compare [Screen before action] to [Screen after action] to see what changed since the last step; {tail}"
        )
    } else {
        format!("[Screen after action] is the current full-screen capture; {tail}")
    };
    let order = if has_previous_raw {
        format!(
            "Order: (1) {SLOT_SCREEN_BEFORE_ACTION} (2) {SLOT_SCREEN_AFTER_ACTION} (3) {SLOT_SCREEN_ANNOTATED} (4) {SLOT_SCREEN_ZOOMED_TOP} (5) {SLOT_SCREEN_ZOOMED_BOTTOM} (6) {SLOT_SCREEN_ZOOMED_POINTER}. {hint}"
        )
    } else {
        format!(
            "Order: (1) {SLOT_SCREEN_AFTER_ACTION} (2) {SLOT_SCREEN_ANNOTATED} (3) {SLOT_SCREEN_ZOOMED_TOP} (4) {SLOT_SCREEN_ZOOMED_BOTTOM} (5) {SLOT_SCREEN_ZOOMED_POINTER}. {hint}"
        )
    };
    format!("{CUR_SCREEN_TAG} {order}\n")
}

fn assemble_cur_screen_base64(cap: &ScreenCaptureResult) -> Vec<String> {
    let mut out = Vec::with_capacity(6);
    if let Some(p) = &cap.inject_previous_raw_jpeg {
        out.push(screen::encode_image_to_base64(p));
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
                let has_previous_raw = cap.inject_previous_raw_jpeg.is_some();
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
    }
}
