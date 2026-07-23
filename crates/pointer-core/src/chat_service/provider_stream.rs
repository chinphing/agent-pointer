use super::json_tool_retries::output_length_retry_supplement;

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
) -> String {
    if is_gateway_error(err) {
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
