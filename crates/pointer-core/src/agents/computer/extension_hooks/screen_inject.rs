//! `message_loop_prompts_after` — `_10_computer_screen_inject` (Python `agents/computer/extensions/...` analogue).

use crate::agents::AgentProfile;
use crate::extensions::{
    new_extension_message_id, ExtensionRegistry, MessageLoopPromptsAfterContext,
    MessageLoopPromptsAfterHook,
};
use crate::models::{ChatMessage, Role, StreamEvent};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

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

        match ctx.computer_state.capture_and_annotate().await {
            Ok((png, monitor)) => {
                emit_screen_notice_update(
                    ctx,
                    notice_id,
                    format!(
                        "【桌面】已截图并完成标注（{}×{}），模型已收到当前画面。",
                        monitor.width, monitor.height
                    ),
                );
                let b64 = super::super::screen::encode_image_to_base64(&png);
                let text = format!(
                    "[CUR_SCREEN] Annotated desktop; numbered overlays mark UI regions. Prefer tools that use overlay indices (e.g. click_index).\nGlobal screen bounds (px): left={} top={} width={} height={}. Normalized coordinate tools use 0–1000 (qwen).\n",
                    monitor.left, monitor.top, monitor.width, monitor.height
                );
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
                    images_base64: Some(vec![b64]),
                });
            }
            Err(e) => {
                log::warn!("computer screen capture/annotate failed: {:#}", e);
                emit_screen_notice_update(
                    ctx,
                    notice_id,
                    format!("【桌面】截图或标注失败：{e}"),
                );
                ctx.messages.push(ChatMessage {
                    id: new_extension_message_id("screen_inject"),
                    role: Role::User,
                    content: format!(
                        "[CUR_SCREEN] Screen capture or UI annotation failed: {e}\nYou cannot rely on a fresh desktop image this turn. Suggest checking screen permissions, annotation service reachability (annotateApiBase / COMPUTER_ANNOTATE_API_BASE), or retrying."
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
                });
            }
        }
        Ok(())
    }
}
