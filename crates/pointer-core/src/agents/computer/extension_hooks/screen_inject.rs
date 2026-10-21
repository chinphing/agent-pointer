//! `message_loop_prompts_after` — `_10_computer_screen_inject` (Python `agents/computer/extensions/...` analogue).
//!
//! OS / locale / **calendar date** for the model live in the **last slice** of merged **`system`**
//! text (`chat_service::prompts::push_env_to_cacheable`, before `before_main_llm_call`).
//! **Full date and time at capture** is prefixed on this hook’s `[CUR_SCREEN]` **`user`** message
//! (`crate::env_prompt::format_local_wall_clock_full`).

use crate::agents::computer::capture_debug;
use crate::agents::computer::screen;
use crate::agents::computer::screen_overlay::{
    SLOT_SCREEN_CURRENT,
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

/// Clear vision payloads from ephemeral inject rows already queued this round.
///
/// Authoritative session history is read-only for hooks and does not persist
/// `[CUR_SCREEN]` / `images_base64`; this only sanitizes `injected_tail` before
/// appending the latest screen message.
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
///
/// Only the current screen: the before/after comparison is the verify stage's
/// job, and its verdict reaches the model as the `verify:` suffix on
/// `[Recent desktop tool calls]`.
fn slot_labels() -> Vec<&'static str> {
    vec![SLOT_SCREEN_CURRENT]
}

fn build_cur_screen_preamble() -> String {
    let cite = "Each screenshot below is preceded by its slot label on its own line. Treat only what you see in that labeled image as ground truth. Do not invent UI from task text or prior turns. Do not write internal checklists, prompt block labels (`[Recent desktop tool calls]`, `[Screen current]`, `[Environment]`, …), or tool-call transcripts in assistant message text — those blocks are context addressed to you, not output for the user. When the user must see a reply (question, blockage, completion), write plain text in **content** in the same turn — reasoning alone is invisible to the user.";
    format!(
        "{CUR_SCREEN_TAG} Labeled image this turn: {SLOT_SCREEN_CURRENT}. {cite} \
         Text below includes **Pointer position** and **Nearby overlay reference bboxes** (10 nearest the pointer; logical-pixel rects). \
         **Verify:** host records outcomes on history rows — read the `verify:` suffix on `[Recent desktop tool calls]`. \
         **Position:** for anything you cannot point at reliably, call `locate_element` and pass its `x`/`y` through unchanged.\n"
    )
}

fn assemble_cur_screen_base64(cap: &ScreenCaptureResult) -> Vec<String> {
    let mut out = Vec::with_capacity(2);
    out.push(screen::encode_image_to_base64(&cap.screenshot_jpeg));
    out
}

fn assemble_cur_screen_payload(cap: &ScreenCaptureResult) -> (Vec<String>, Vec<String>) {
    let labels: Vec<String> = slot_labels()
        .into_iter()
        .map(str::to_string)
        .collect();
    let images = assemble_cur_screen_base64(cap);
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

fn emit_screen_thread_notice(
    ctx: &MessageLoopPromptsAfterContext<'_>,
    message_id: String,
    content: String,
) {
    let Some(tx) = ctx.stream else {
        return;
    };
    let _ = tx.send(StreamEvent::InjectedAssistantMessage {
        conversation_id: ctx.conversation_id.to_string(),
        message_id,
        content,
    });
}

fn emit_screen_notice_update(
    ctx: &MessageLoopPromptsAfterContext<'_>,
    message_id: String,
    content: String,
) {
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
    fn override_key(&self) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("_10_computer_screen_inject")
    }

    fn sort_key(&self) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("_10_computer_screen_inject")
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

        match ctx
            .computer_state
            .capture_and_annotate(ctx.conversation_id)
            .await
        {
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
                let annotated_rel = capture_debug::save_computer_capture_debug(
                    ctx.conversation_id,
                    dump_prefix,
                    &cap,
                );
                if let (Some(tx), Some(mid)) = (ctx.stream, ctx.round_assistant_message_id.as_ref())
                {
                    if let Some(rel) = annotated_rel {
                        let _ = tx.send(StreamEvent::AssistantRoundScreen {
                            conversation_id: ctx.conversation_id.to_string(),
                            message_id: mid.clone(),
                            annotated_rel_path: rel,
                        });
                    }
                }
                emit_screen_notice_update(ctx, notice_id, DESKTOP_NOTICE_READY.to_string());
                strip_images_from_prior_messages(ctx.injected_tail.as_mut_slice());
                let (image_slot_labels, images) = assemble_cur_screen_payload(&cap);
                let mut text = cur_screen_clock_prefix();
                if let Some(label) = ctx.computer_state.locked_goal_label(ctx.conversation_id) {
                    text.push_str(&format!("Locked goal: {label}\n\n"));
                }
                text.push_str(&build_cur_screen_preamble());
                if let Some(block) = ctx
                    .computer_state
                    .recent_actions_prompt_block(ctx.conversation_id)
                {
                    text.push_str("\n\n");
                    text.push_str(&block);
                    text.push('\n');
                }
                if let Some(ref anchor) = cap.mouse_neighbor_reference_text {
                    text.push_str("\n\n");
                    text.push_str(anchor);
                }
                ctx.injected_tail.push(ChatMessage {
                    turn_elapsed_ms: None,
                    id: new_extension_message_id("screen_inject"),
                    role: Role::User,
                    content: text,
                    status: "done".into(),
                    created_at: crate::extensions::now_ms(),
                    tool_calls: None,
                    tool_call_id: None,
                    tool_name: None,
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
                    anchor_message_id: None,
                    trace_id: None,
                    task_id: None,
                    spawn_depth: None,
                    agent_chain: None,
                });
            }
            Err(e) => {
                log::warn!("computer screenshot processing failed: {:#}", e);
                emit_screen_notice_update(ctx, notice_id, DESKTOP_NOTICE_FAILED.to_string());
                strip_images_from_prior_messages(ctx.injected_tail.as_mut_slice());
                ctx.injected_tail.push(ChatMessage {
                    turn_elapsed_ms: None,
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
                    tool_name: None,
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
                    anchor_message_id: None,
                    trace_id: None,
                    task_id: None,
                    spawn_depth: None,
                    agent_chain: None,
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
            turn_elapsed_ms: None,
            id: "m1".into(),
            role: Role::User,
            content: "[CUR_SCREEN] test".into(),
            status: "done".into(),
            created_at: 0,
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
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
            anchor_message_id: None,
            trace_id: None,
            task_id: None,
            spawn_depth: None,
            agent_chain: None,
        }
    }

    fn dummy_cap(has_before: bool) -> ScreenCaptureResult {
        use crate::agents::computer::screen::MonitorInfo;
        ScreenCaptureResult {
            raw_unmarked_jpeg: vec![1, 2],
            screenshot_jpeg: vec![3, 4],
            mouse_neighbor_reference_text: None,
            monitor: MonitorInfo::new(0, 0, 100, 100),
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
    fn slot_labels_match_image_count_primary() {
        let cap = dummy_cap(true);
        let (labels, images) = assemble_cur_screen_payload(&cap);
        assert_eq!(labels.len(), 1);
        assert_eq!(labels.len(), images.len());
    }

    #[test]
    fn slot_layout_without_before() {
        let cap = dummy_cap(false);
        let (labels, images) = assemble_cur_screen_payload(&cap);
        assert_eq!(labels.len(), 1);
        assert_eq!(labels, vec![SLOT_SCREEN_CURRENT]);
        assert_eq!(labels.len(), images.len());
    }

    /// Only the current screen is injected. Before/after comparison belongs to
    /// the verify stage, whose verdict reaches the model as `verify:` text.
    #[test]
    fn cur_screen_has_no_before_frame() {
        let cap = dummy_cap(true);
        let (labels, images) = assemble_cur_screen_payload(&cap);
        assert_eq!(
            labels.first().map(String::as_str),
            Some(SLOT_SCREEN_CURRENT)
        );
        assert_eq!(labels.len(), images.len());
    }

    #[test]
    fn preamble_mentions_labeled_images() {
        let t = build_cur_screen_preamble();
        assert!(t.contains("Labeled image"));
        assert!(t.contains("locate_element"));
        assert!(t.contains("verify:"));
        assert!(t.contains(SLOT_SCREEN_CURRENT));
        // The before frame is gone: verify owns before/after comparison.
        assert!(!t.contains("before action"));
        assert!(!t.contains("N–target relation"));
    }
}
