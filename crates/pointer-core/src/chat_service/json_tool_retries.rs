use crate::models::{ChatMessage, Role, StreamEvent};

use super::emit::emit;
use super::util::{new_id, now_ms};
use super::StreamTx;

pub(crate) fn output_length_retry_supplement(max_tokens: u32, finish_reason: &str) -> String {
    format!(
        "\n\n【输出长度】本回合因 **输出 token 上限** 被截断（finish_reason={finish_reason}，配置 max_tokens≈{max_tokens}）。\
         请**缩小**本回合 payload：拆分 `file_edit` / `file_write`、缩短单次参数体积，分多轮完成。"
    )
}

pub(crate) fn push_injected_format_retry_turn(
    stream: &StreamTx,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    hint: String,
) {
    let retry_id = new_id("fmt_retry");
    emit(
        stream,
        StreamEvent::InjectedUserMessage {
            conversation_id: conversation_id.to_string(),
            message_id: retry_id.clone(),
            content: hint.clone(),
        },
    );
    history.push(ChatMessage {
        id: retry_id,
        role: Role::User,
        content: hint,
        status: "done".into(),
        created_at: now_ms(),
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
            });
}
