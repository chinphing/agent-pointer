use crate::models::{ChatMessage, Role, StreamEvent};

use super::emit::emit;
use super::util::{new_id, now_ms};
use super::StreamTx;

fn is_output_length_limited_finish_reason(finish_reason: &str) -> bool {
    matches!(
        finish_reason.trim().to_ascii_lowercase().as_str(),
        "length" | "max_tokens"
    )
}

pub(crate) fn output_length_retry_supplement(max_tokens: u32, finish_reason: &str) -> String {
    format!(
        "\n\n【输出长度】本回合因 **输出 token 上限** 被截断（finish_reason={finish_reason}，配置 max_tokens≈{max_tokens}）。\
         请**缩小**本回合 JSON：拆分 `file:edit` / `file:write`、缩短 `tool_args` 里的长字符串，分多轮完成；仍须输出**完整闭合**的单一 JSON 对象。"
    )
}

/// When tools appendix is enabled but this turn produced no executable tool call, inject a user-line
/// for the next model turn. Public format rules are already in the system prompts each round via
/// [`crate::agents::rendered_communication_public_inject`] / [`crate::agents::expand_agent_prompt_placeholders`]; this message only states the failure and JSON escaping hints.
pub(crate) fn json_tool_empty_calls_retry_message(
    diag: &crate::json_tool_caller::JsonToolFinishDiagnostics,
    tools_appendix_enabled: bool,
    finish_reason: &str,
    max_tokens: u32,
) -> Option<String> {
    if !tools_appendix_enabled {
        return None;
    }
    const JSON_FORMAT_RETRY_HINT: &str = "请确保 JSON 中特殊字符都经过转义后再输出，包括换行符、制表符、双引号。\n\
同时请严格按照输出 JSON 格式要求输出，最小输出示例：\n\
{\"thoughts\":\"简要推理\",\"headline\":\"短标题\",\"tool_name\":\"response\",\"tool_args\":{\"text\":\"给用户看的完整回复\"}}";

    let intro = if is_output_length_limited_finish_reason(finish_reason) {
        format!(
            "【环境反馈】本回合输出因达到 max_tokens 上限（finish_reason={finish_reason}）被截断，JSON 不完整。\
             请缩小本回合输出并分多轮完成。\n{JSON_FORMAT_RETRY_HINT}"
        )
    } else if diag.attempted_tool_json {
        if diag.fragment_complete {
            let detail = diag
                .parse_error
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("无法解析为合法 JSON 工具信封");
            format!("【环境反馈】本回合 JSON 格式异常：{detail}。\n{JSON_FORMAT_RETRY_HINT}")
        } else {
            format!(
                "【环境反馈】本回合 JSON 不完整（流结束前未形成可解析的完整对象）。\n{JSON_FORMAT_RETRY_HINT}"
            )
        }
    } else {
        format!("【环境反馈】本回合 JSON 格式异常或未输出 JSON 格式。\n{JSON_FORMAT_RETRY_HINT}")
    };

    let mut body = intro;
    if is_output_length_limited_finish_reason(finish_reason) {
        body.push_str(&output_length_retry_supplement(max_tokens, finish_reason));
    }
    if let Some(head) = diag
        .consumed_fragment_head
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        body.push_str("\n\n【你上一回合输出的开头片段（供对照修正）】\n");
        body.push_str(head);
    }
    Some(body)
}

pub(crate) fn json_tool_envelope_batch_retry_message(err: &str) -> String {
    format!(
        "【环境反馈】本回合工具调用组合不符合协议：{err}\n\n\
         当使用 `sidecar_tools` 数组时：仅允许将白名单侧车工具（例如 `task_board:patch`）放在其中每一项；根级必须恰好保留一对主工具 `tool_name`/`tool_args`，且不得仅为侧车工具。\n\
         若无 `sidecar_tools`，则仍只使用根级单工具。请按系统提示中的 JSON 约定重新输出完整的 JSON 对象。"
    )
}

/// Drop a failed non-JSON assistant turn from API history so the model is not trained on plain prose.
pub(crate) fn rollback_failed_json_assistant_turn(history: &mut Vec<ChatMessage>, assistant_id: &str) {
    if history.last().is_some_and(|m| {
        matches!(m.role, Role::Assistant) && m.id == assistant_id
    }) {
        history.pop();
        log::info!(
            "rolled back non-JSON assistant turn from API history (assistant_id={assistant_id})"
        );
    }
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
        agent_id: None,
        agent_name: None,
        agent_trace: None,
        images_base64: None,
        computer_round_screen_rel_path: None,
    });
}
