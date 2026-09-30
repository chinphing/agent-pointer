use super::json_tool_retries::output_length_retry_supplement;
use crate::models::ChatMessage;
use std::time::Duration;

pub(crate) const RATE_LIMIT_RETRY_MARKER: &str = "<!-- pointer-rate-limit-retry -->";
const RATE_LIMIT_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
];

pub(crate) fn is_http_429(err: &anyhow::Error) -> bool {
    let s = err.to_string().to_ascii_lowercase();
    s.contains("http 429") || s.contains("too many requests") || s.contains("limit_burst_rate")
}

/// Return the next bounded backoff for a rate-limited conversation or sub-agent history.
pub(crate) fn rate_limit_retry_delay(
    err: &anyhow::Error,
    history: &[ChatMessage],
) -> Option<Duration> {
    if !is_http_429(err) {
        return Some(Duration::ZERO);
    }
    let prior_retries = history
        .iter()
        .filter(|message| message.content.contains(RATE_LIMIT_RETRY_MARKER))
        .count();
    RATE_LIMIT_RETRY_DELAYS.get(prior_retries).copied()
}

/// Stream/HTTP failures where a retry might help (includes all HTTP gateway errors).
pub(crate) fn is_recoverable_provider_stream_error(err: &anyhow::Error) -> bool {
    let s = err.to_string().to_ascii_lowercase();
    s.contains("decoding response body")
        || s.contains("error decoding")
        || s.contains("unexpected eof")
        || s.contains("connection reset")
        || s.contains("broken pipe")
        || s.contains("incomplete message")
        || s.contains("body completed")
        || s.contains("http 429")
        || s.contains("http 500")
        || s.contains("http 502")
        || s.contains("http 503")
        || s.contains("http 504")
}

/// Gateway-level HTTP errors (upstream unreachable or misbehaving).
pub(crate) fn is_gateway_error(err: &anyhow::Error) -> bool {
    let s = err.to_string().to_ascii_lowercase();
    s.contains("http 502") || s.contains("http 503") || s.contains("http 504")
}

pub(crate) fn provider_stream_recoverable_retry_message(
    err: &anyhow::Error,
    max_tokens: u32,
    rate_limit_delay: Option<Duration>,
) -> String {
    if let Some(delay) = rate_limit_delay.filter(|delay| !delay.is_zero()) {
        format!(
            "【环境反馈】模型服务触发限流（{err}）。已等待 {} 秒后重试。\
             若仍失败，请减少并发请求或稍后再试。\n\n{RATE_LIMIT_RETRY_MARKER}",
            delay.as_secs()
        )
    } else if is_gateway_error(err) {
        format!(
            "【环境反馈】上游服务暂时不可用（{err}），正在重试。\n\n\
             请勿修改输入，等待服务恢复即可。"
        )
    } else {
        format!(
            "【环境反馈】本回合模型输出异常（{err}），常见于输出过长或流传输中断。\n\n\
             请缩小本回合 payload（拆分编辑、减少超长参数），按原生工具调用协议重试。\
             当前 max_tokens≈{max_tokens}。{length_hint}",
            length_hint = output_length_retry_supplement(max_tokens, "length")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ChatMessage, Role};

    fn retry_message() -> ChatMessage {
        ChatMessage {
            id: "retry".into(),
            role: Role::User,
            content: RATE_LIMIT_RETRY_MARKER.into(),
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
        }
    }

    #[test]
    fn rate_limit_uses_bounded_backoff_schedule() {
        let err = anyhow::anyhow!("HTTP 429 limit_burst_rate");
        assert_eq!(
            rate_limit_retry_delay(&err, &[]),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            rate_limit_retry_delay(&err, &[retry_message()]),
            Some(Duration::from_secs(15))
        );
        assert_eq!(
            rate_limit_retry_delay(&err, &[retry_message(), retry_message()]),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            rate_limit_retry_delay(&err, &[retry_message(), retry_message(), retry_message()]),
            None
        );
    }
}
