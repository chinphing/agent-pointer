//! When conversation history grows past a rough character budget, replace an older prefix
//! with a single user message containing an LLM-generated summary (see settings).

use crate::models::{ChatMessage, ModelSettings, Role, StreamEvent};
use crate::provider::OpenAIProvider;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

const MAX_PREFIX_CHARS_FOR_API: usize = 100_000;
const MAX_SNIPPET_CHARS: usize = 2_500;

type StreamTx = UnboundedSender<StreamEvent>;

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn estimate_message_payload_chars(msgs: &[ChatMessage]) -> usize {
    let mut n = 0usize;
    for m in msgs {
        n += m.content.chars().count();
        n += m.reasoning.as_deref().map(str::len).unwrap_or(0);
        n += m.error_message.as_deref().map(str::len).unwrap_or(0);
        if let Some(tcs) = &m.tool_calls {
            for t in tcs {
                n += t.id.len() + t.name.len() + t.arguments.len();
                n += t.result.as_deref().map(str::len).unwrap_or(0);
                n += t.error.as_deref().map(str::len).unwrap_or(0);
            }
        }
        if let Some(id) = &m.tool_call_id {
            n += id.len();
        }
    }
    n
}

/// Start index of the Nth user message from the end (`N >= 1`). Returns 0 if fewer than N users exist.
fn find_split_at_user_boundary(msgs: &[ChatMessage], keep_last_n_users: usize) -> usize {
    if keep_last_n_users == 0 || msgs.is_empty() {
        return 0;
    }
    let mut seen = 0usize;
    for i in (0..msgs.len()).rev() {
        if matches!(msgs[i].role, Role::User) {
            seen += 1;
            if seen == keep_last_n_users {
                return i;
            }
        }
    }
    0
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let c: usize = s.chars().count();
    if c <= max_chars {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max_chars).collect::<String>())
    }
}

fn format_prefix_for_summary(msgs: &[ChatMessage]) -> String {
    let mut blocks = Vec::with_capacity(msgs.len());
    for m in msgs {
        let head = match m.role {
            Role::System => "系统",
            Role::User => "用户",
            Role::Assistant => "助手",
            Role::Tool => "工具结果",
        };
        let mut body = truncate_chars(&m.content, MAX_SNIPPET_CHARS);
        if let Some(r) = &m.reasoning {
            if !r.is_empty() {
                body.push_str("\n[推理片段] ");
                body.push_str(&truncate_chars(r, 800));
            }
        }
        if let Some(tcs) = &m.tool_calls {
            for t in tcs {
                body.push_str(&format!(
                    "\n[工具 {} 参数] {}",
                    t.name,
                    truncate_chars(&t.arguments, 1200)
                ));
                if let Some(res) = &t.result {
                    body.push_str(&format!(
                        "\n[工具 {} 输出] {}",
                        t.name,
                        truncate_chars(res, MAX_SNIPPET_CHARS)
                    ));
                }
                if let Some(err) = &t.error {
                    body.push_str(&format!("\n[工具 {} 错误] {}", t.name, truncate_chars(err, 800)));
                }
            }
        }
        if matches!(m.role, Role::Tool) {
            if let Some(id) = &m.tool_call_id {
                body.push_str(&format!("\n(tool_call_id: {id})"));
            }
        }
        blocks.push(format!("--- {head} ---\n{body}"));
    }
    let mut out = blocks.join("\n\n");
    if out.chars().count() > MAX_PREFIX_CHARS_FOR_API {
        let take = MAX_PREFIX_CHARS_FOR_API.saturating_sub(80);
        out = format!(
            "{}\n\n…（前缀过长，已截断至约 {} 字符用于摘要）",
            truncate_chars(&out, take),
            MAX_PREFIX_CHARS_FOR_API
        );
    }
    out
}

const SUMMARY_SYSTEM: &str = r#"你是对话压缩助手。用户将提供一段较早的多轮对话（含可能的工具调用与结果摘录）。
请用中文输出结构化摘要，尽量保留：
1) 用户的原始目标与约束
2) 已做出的关键决策、结论、修改过的文件路径或命令
3) 未解决的问题、待办、错误信息
4) 重要的数字、配置名、API 名称

不要编造未出现的事实；不确定请标注「原文未明确」。输出控制在简洁的若干段落或要点列表，不要寒暄。"#;

fn summary_fallback_notice() -> String {
    "【历史对话摘要（自动压缩）】\n\n（摘要请求失败或已取消：较早对话已移除。若需前文信息请简要重述你的目标与关键上下文。）".into()
}

fn new_summary_user_message(body: String) -> ChatMessage {
    ChatMessage {
        id: format!("ctx_{}", uuid::Uuid::new_v4().simple()),
        role: Role::User,
        content: body,
        status: "done".into(),
        created_at: now_ms(),
        tool_calls: None,
        tool_call_id: None,
        error_message: None,
        reasoning: None,
        agent_id: None,
        agent_name: None,
        agent_trace: None,
    }
}

async fn compress_history_inner(
    history: &mut Vec<ChatMessage>,
    settings: &ModelSettings,
    provider: &OpenAIProvider,
    conversation_id: &str,
    stream: &StreamTx,
    cancel: CancellationToken,
    force_ignore_char_budget: bool,
    emit_history_replaced: bool,
) -> bool {
    if !settings.context_compression_enabled {
        return false;
    }
    let keep_users = settings.context_keep_recent_user_turns.max(1);
    let budget = settings.context_budget_chars.max(4096);

    let est = estimate_message_payload_chars(history);
    if !force_ignore_char_budget && est <= budget as usize {
        return false;
    }

    let split = find_split_at_user_boundary(history, keep_users as usize);
    if split == 0 {
        log::info!("context compression skipped: no safe user boundary");
        return false;
    }

    let prefix = &history[..split];
    if prefix.is_empty() {
        return false;
    }

    let suffix = history[split..].to_vec();
    let formatted = format_prefix_for_summary(prefix);

    let input = ChatMessage {
        id: format!("sum_in_{}", uuid::Uuid::new_v4().simple()),
        role: Role::User,
        content: formatted,
        status: "done".into(),
        created_at: now_ms(),
        tool_calls: None,
        tool_call_id: None,
        error_message: None,
        reasoning: None,
        agent_id: None,
        agent_name: None,
        agent_trace: None,
    };

    let max_tok = settings.context_summary_max_tokens.max(128);
    let summary_prefix = if force_ignore_char_budget {
        "【历史对话摘要（工具轮次触发的自动压缩）】"
    } else {
        "【历史对话摘要（自动压缩）】"
    };
    let summary_body = match provider
        .chat_once(
            std::slice::from_ref(&input),
            &[SUMMARY_SYSTEM.to_string()],
            Vec::new(),
            cancel.clone(),
            Some(max_tok),
        )
        .await
    {
        Ok(text) => {
            let t = text.trim();
            if t.is_empty() {
                log::warn!("context summary returned empty; using fallback notice");
                summary_fallback_notice()
            } else {
                format!("{summary_prefix}\n\n{t}")
            }
        }
        Err(e) => {
            log::warn!("context summary LLM call failed: {e}; using fallback notice");
            summary_fallback_notice()
        }
    };

    let summary_msg = new_summary_user_message(summary_body);
    let mut new_hist = Vec::with_capacity(1 + suffix.len());
    new_hist.push(summary_msg);
    new_hist.extend(suffix);
    *history = new_hist;

    if emit_history_replaced {
        let _ = stream.send(StreamEvent::HistoryReplaced {
            conversation_id: conversation_id.to_string(),
            messages: history.clone(),
        });
    }
    true
}

/// When history exceeds char budget, summarize prefix. Emits `HistoryReplaced` when successful.
pub async fn maybe_compress_history(
    history: &mut Vec<ChatMessage>,
    settings: &ModelSettings,
    provider: &OpenAIProvider,
    conversation_id: &str,
    stream: &StreamTx,
    cancel: CancellationToken,
) {
    let _ = compress_history_inner(
        history,
        settings,
        provider,
        conversation_id,
        stream,
        cancel,
        false,
        true,
    )
    .await;
}

/// After tool-round limit: try summarization even if under char budget. Returns whether history changed.
pub async fn maybe_compress_after_tool_round_limit(
    history: &mut Vec<ChatMessage>,
    settings: &ModelSettings,
    provider: &OpenAIProvider,
    conversation_id: &str,
    stream: &StreamTx,
    cancel: CancellationToken,
    emit_history_replaced: bool,
) -> bool {
    compress_history_inner(
        history,
        settings,
        provider,
        conversation_id,
        stream,
        cancel,
        true,
        emit_history_replaced,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> ChatMessage {
        ChatMessage {
            id: "u".into(),
            role: Role::User,
            content: s.into(),
            status: "done".into(),
            created_at: 0,
            tool_calls: None,
            tool_call_id: None,
            error_message: None,
            reasoning: None,
            agent_id: None,
            agent_name: None,
            agent_trace: None,
        }
    }

    #[test]
    fn split_keeps_last_n_users() {
        let msgs = vec![u("a"), u("b"), u("c")];
        assert_eq!(find_split_at_user_boundary(&msgs, 1), 2);
        assert_eq!(find_split_at_user_boundary(&msgs, 2), 1);
        assert_eq!(find_split_at_user_boundary(&msgs, 3), 0);
    }

    #[test]
    fn split_fewer_users_than_keep_returns_zero() {
        let msgs = vec![u("only")];
        assert_eq!(find_split_at_user_boundary(&msgs, 2), 0);
    }
}
