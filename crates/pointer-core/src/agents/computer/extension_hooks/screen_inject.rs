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

/// How old a pipeline capture may be before `[CUR_SCREEN]` refuses to reuse it.
///
/// The verify step of the previous round leaves its after-frame in the cache, and
/// `[CUR_SCREEN]` wants exactly that frame. Measured gap on a normal round is ~5s
/// (capture + verify); under CPU contention it reached ~27s, so the bound has to
/// be generous. Anything older means the turn sat idle and the screen may have
/// moved — capture fresh instead of showing the model a stale picture.
const CUR_SCREEN_CACHE_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(120);

/// Thread notice while capture runs (user-visible).
const DESKTOP_NOTICE_PROCESSING: &str = "【桌面】截图中";
const DESKTOP_NOTICE_READY: &str = "【桌面】已更新当前画面。";
/// User-facing failure copy — do not echo internal errors.
const DESKTOP_NOTICE_FAILED: &str =
    "【桌面】截图失败，请检查屏幕录制权限后重试。";

fn cur_screen_failure_model_message() -> String {
    format!(
        "{tag} Screenshot capture failed this turn.\n\
         You cannot rely on a fresh desktop image. \
         The user may need to grant screen recording permission; suggest retrying after that.",
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

/// `[CUR_SCREEN]` preamble text, byte-identical to what production injects.
///
/// `pub` for `tests/computer_e2e`: the benchmark must feed the main LLM the
/// same preamble production does, otherwise a hint measured there says nothing
/// about the shipped behaviour.
#[doc(hidden)]
/// Preamble for the `[CUR_SCREEN]` message.
///
/// Takes the capture so the screenshot's pixel dimensions can be stated here
/// rather than baked into a prompt: the size is runtime data, and the `bbox`
/// contract in `communication.md` ("Locating targets") refers to it instead of naming a
/// number that would go stale on the next monitor change.
pub fn build_cur_screen_preamble(cap: &ScreenCaptureResult) -> String {
    let cite = "Each screenshot below is preceded by its slot label on its own line. Treat only what you see in that labeled image as ground truth. Do not invent UI from task text or prior turns. Do not write internal checklists, prompt block labels (`[Recent desktop tool calls]`, `[Screen current]`, `[Environment]`, …), or tool-call transcripts in assistant message text — those blocks are context addressed to you, not output for the user. When the user must see a reply (question, blockage, completion), write plain text in **content** in the same turn — reasoning alone is invisible to the user.";
    format!(
        "{CUR_SCREEN_TAG} Labeled image this turn: {SLOT_SCREEN_CURRENT}. This screenshot is \
         {w}×{h} pixels — use these dimensions for the `bbox` conversion described in \
         **Locating targets**. {cite} \
         **Verify:** host records outcomes on history rows — read the `verify:` suffix on `[Recent desktop tool calls]`.\n",
        w = cap.monitor.width.max(1),
        h = cap.monitor.height.max(1),
    )
}

fn assemble_cur_screen_base64(cap: &ScreenCaptureResult) -> Vec<String> {
    let mut out = Vec::with_capacity(1);
    out.push(screen::encode_image_to_base64(&cap.screenshot_jpeg));
    out
}

/// `(slot_labels, base64_images)` for the `[CUR_SCREEN]` message — same pair
/// production attaches, so `image_slot_labels` stays consistent.
#[doc(hidden)]
pub fn assemble_cur_screen_payload(cap: &ScreenCaptureResult) -> (Vec<String>, Vec<String>) {
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

        let capture = match ctx.computer_state.fresh_pipeline_capture(
            ctx.conversation_id,
            CUR_SCREEN_CACHE_MAX_AGE,
        ) {
            Some(cap) => Ok((cap, None)),
            None => {
                ctx.computer_state
                    .capture_screen(ctx.conversation_id)
                    .await
            }
        };

        match capture {
            Ok((cap, refreshed_monitor_id)) => {
                // Hand this frame to the tool pass / verify so they do not shoot the
                // same screen again. Bounded by `CUR_SCREEN_CACHE_MAX_AGE` above.
                ctx.computer_state
                    .store_pipeline_after_capture(ctx.conversation_id, cap.clone());
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
                let capture_rel = capture_debug::save_computer_capture_debug(
                    ctx.conversation_id,
                    dump_prefix,
                    &cap,
                );
                if let (Some(tx), Some(mid)) = (ctx.stream, ctx.round_assistant_message_id.as_ref())
                {
                    if let Some(rel) = capture_rel {
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
                text.push_str(&build_cur_screen_preamble(&cap));
                if let Some(block) = ctx
                    .computer_state
                    .recent_actions_prompt_block(ctx.conversation_id)
                {
                    text.push_str("\n\n");
                    text.push_str(&block);
                    text.push('\n');
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
                log::warn!("computer screenshot failed: {:#}", e);
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

    fn dummy_cap() -> ScreenCaptureResult {
        use crate::agents::computer::screen::MonitorInfo;
        ScreenCaptureResult {
            raw_unmarked_jpeg: vec![1, 2],
            screenshot_jpeg: vec![3, 4],
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
        let cap = dummy_cap();
        let (labels, images) = assemble_cur_screen_payload(&cap);
        assert_eq!(labels.len(), 1);
        assert_eq!(labels.len(), images.len());
    }

    #[test]
    fn slot_layout_is_current_screen_only() {
        let cap = dummy_cap();
        let (labels, images) = assemble_cur_screen_payload(&cap);
        assert_eq!(labels.len(), 1);
        assert_eq!(labels, vec![SLOT_SCREEN_CURRENT]);
        assert_eq!(labels.len(), images.len());
    }

    /// Only the current screen is injected. Before/after comparison belongs to
    /// the verify stage, whose verdict reaches the model as `verify:` text.
    #[test]
    fn cur_screen_has_no_before_frame() {
        let cap = dummy_cap();
        let (labels, images) = assemble_cur_screen_payload(&cap);
        assert_eq!(
            labels.first().map(String::as_str),
            Some(SLOT_SCREEN_CURRENT)
        );
        assert_eq!(labels.len(), images.len());
    }

    /// Minimal capture for preamble tests: only `monitor` is read.
    fn sample_cap() -> ScreenCaptureResult {
        ScreenCaptureResult {
            raw_unmarked_jpeg: Vec::new(),
            screenshot_jpeg: Vec::new(),
            monitor: crate::agents::computer::vision::screen::MonitorInfo::new(0, 0, 1920, 1080),
        }
    }

    #[test]
    fn preamble_mentions_labeled_images() {
        let t = build_cur_screen_preamble(&sample_cap());
        assert!(t.contains("Labeled image"));
        assert!(t.contains("verify:"));
        assert!(t.contains(SLOT_SCREEN_CURRENT));
        // The screenshot's pixel size is stated here, not in a prompt: it is
        // runtime data and the `bbox` contract refers to it.
        assert!(t.contains("1920×1080"), "missing dimensions: {t}");
        // The before frame is gone: verify owns before/after comparison.
        assert!(!t.contains("before action"));
        assert!(!t.contains("N–target relation"));
    }

    #[test]
    fn preamble_advertises_no_pixel_coordinates() {
        // `locate_element` was removed (676fec91) and
        // `mouse_neighbor_reference_text` is never populated, so the old
        // sentence promised "Pointer position" and "logical-pixel rects" that
        // did not exist — and told the model to answer in `x`/`y` pixels.
        let t = build_cur_screen_preamble(&sample_cap());
        assert!(!t.contains("locate_element"), "dead tool: {t}");
        assert!(!t.contains("logical-pixel"), "pixel hint: {t}");
        assert!(!t.contains("Pointer position"), "phantom data: {t}");
        assert!(!t.contains("Nearby overlay"), "phantom data: {t}");
        assert!(!t.contains("x`/`y"), "pixel-style answer hint: {t}");
    }
}
