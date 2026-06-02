//! When conversation history grows past a rough character budget, replace an older prefix
//! with a single user message containing an LLM-generated summary (see settings).

use crate::agent_instance_scope::AgentInstanceScope;
use crate::models::{ChatMessage, ContextCompressionInfo, ModelSettings, Role, StreamEvent};
use crate::provider::OpenAIProvider;
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

const MAX_PREFIX_CHARS_FOR_API: usize = 100_000;
const MAX_USER_SNIPPET_CHARS: usize = 4_000;
const MAX_ASSISTANT_SNIPPET_CHARS: usize = 2_500;
const MAX_TOOL_SNIPPET_CHARS: usize = 2_000;
const MAX_TOOL_ARGS_CHARS: usize = 1_200;
const MAX_TOOL_ERROR_CHARS: usize = 1_000;
const MAX_REASONING_SNIPPET_CHARS: usize = 1_000;

/// Prefix on summary user rows after compression (UI detects this for dedicated styling).
pub const SUMMARY_PREFIX_BUDGET: &str = "[Conversation summary (auto-compression)]";
pub const SUMMARY_PREFIX_TOOL_LIMIT: &str =
    "[Conversation summary (auto-compression after tool rounds)]";

type StreamTx = UnboundedSender<StreamEvent>;

/// Whether compression UI/events target the main thread or an isolated sub-agent loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionScope {
    #[default]
    Main,
    SubAgent,
}

/// Optional anchors for compression UI (main thread vs delegated sub-agent such as `explore`).
#[derive(Debug, Clone, Default)]
pub struct CompressionUiContext {
    pub scope: CompressionScope,
    /// Token reporting scope for this compression LLM call.
    pub agent_scope: Option<AgentInstanceScope>,
    /// Parent assistant message id (sub-agent trace anchoring).
    pub message_id: Option<String>,
    pub sub_agent_id: Option<String>,
    pub sub_agent_name: Option<String>,
    pub task_id: Option<String>,
}

impl CompressionUiContext {
    pub fn main(agent_scope: AgentInstanceScope) -> Self {
        Self {
            scope: CompressionScope::Main,
            agent_scope: Some(agent_scope),
            ..Self::default()
        }
    }

    pub fn sub_agent(
        agent_scope: AgentInstanceScope,
        message_id: &str,
        agent_id: &str,
        agent_name: &str,
        task_id: &str,
    ) -> Self {
        Self {
            scope: CompressionScope::SubAgent,
            agent_scope: Some(agent_scope),
            message_id: Some(message_id.to_string()),
            sub_agent_id: Some(agent_id.to_string()),
            sub_agent_name: Some(agent_name.to_string()),
            task_id: Some(task_id.to_string()),
        }
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn emit_ui_toast(stream: &StreamTx, conversation_id: &str, message: &str, level: &str) {
    let _ = stream.send(StreamEvent::UiToast {
        conversation_id: conversation_id.to_string(),
        message: message.to_string(),
        level: level.to_string(),
    });
}

fn compression_start_toast(ui: &CompressionUiContext) -> String {
    match ui.scope {
        CompressionScope::Main => "对话较长，正在压缩较早记录…".to_string(),
        CompressionScope::SubAgent => {
            let name = ui
                .sub_agent_name
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("子 Agent");
            format!("{name} 子任务内正在压缩较早记录…")
        }
    }
}

fn compression_done_toast(
    ui: &CompressionUiContext,
    dropped: u32,
    keep_users: u32,
    summary_failed: bool,
) -> (String, &'static str) {
    let level = if summary_failed { "warning" } else { "success" };
    let msg = match ui.scope {
        CompressionScope::Main => {
            if summary_failed {
                format!(
                    "摘要生成失败，已丢弃较早 {dropped} 条记录，保留最近 {keep_users} 轮用户消息"
                )
            } else {
                format!(
                    "已压缩较早 {dropped} 条对话为摘要，保留最近 {keep_users} 轮用户消息"
                )
            }
        }
        CompressionScope::SubAgent => {
            let name = ui
                .sub_agent_name
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("子 Agent");
            if summary_failed {
                format!("{name} 子任务：摘要失败，已丢弃较早 {dropped} 条记录")
            } else {
                format!("{name} 子任务：已压缩较早 {dropped} 条记录为摘要")
            }
        }
    };
    (msg, level)
}

fn build_compression_info(
    ui: &CompressionUiContext,
    reason: &str,
    messages_before: usize,
    messages_after: usize,
    dropped: u32,
    keep_users: u32,
) -> ContextCompressionInfo {
    ContextCompressionInfo {
        reason: reason.to_string(),
        messages_before: messages_before as u32,
        messages_after: messages_after as u32,
        dropped_count: dropped,
        keep_recent_user_turns: keep_users,
        scope: match ui.scope {
            CompressionScope::Main => "main".into(),
            CompressionScope::SubAgent => "sub_agent".into(),
        },
        sub_agent_id: ui.sub_agent_id.clone(),
        sub_agent_name: ui.sub_agent_name.clone(),
        task_id: ui.task_id.clone(),
    }
}

pub fn estimate_message_payload_chars(msgs: &[ChatMessage]) -> usize {
    let included = crate::message_context::filter_context_messages(msgs);
    let mut n = 0usize;
    for m in &included {
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

/// Deprecated: use [`crate::message_context::find_split_at_user_boundary`].
pub(crate) fn find_split_at_user_boundary(msgs: &[ChatMessage], keep_last_n_users: usize) -> usize {
    crate::message_context::find_split_at_user_boundary(msgs, keep_last_n_users)
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let c: usize = s.chars().count();
    if c <= max_chars {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max_chars).collect::<String>())
    }
}

fn content_snippet_limit(role: &Role) -> usize {
    match role {
        Role::User => MAX_USER_SNIPPET_CHARS,
        Role::Assistant => MAX_ASSISTANT_SNIPPET_CHARS,
        Role::Tool => MAX_TOOL_SNIPPET_CHARS,
        Role::System => MAX_ASSISTANT_SNIPPET_CHARS,
    }
}

/// Tool results that carry paths, hits, or handoffs deserve a larger excerpt for summarization.
fn tool_output_snippet_limit(tool_name: &str) -> usize {
    let n = tool_name.trim().to_lowercase();
    if n.starts_with("file:grep") || n == "run_subagent" {
        3_500
    } else if n.starts_with("file:read") {
        1_800
    } else if n.starts_with("terminal") || n.starts_with("read_lints") {
        2_500
    } else {
        MAX_TOOL_SNIPPET_CHARS
    }
}

fn format_prefix_for_summary(msgs: &[ChatMessage]) -> String {
    let mut blocks = Vec::with_capacity(msgs.len());
    for m in msgs {
        let head = match m.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        let limit = content_snippet_limit(&m.role);
        let mut body = truncate_chars(&m.content, limit);
        if let Some(r) = &m.reasoning {
            if !r.is_empty() {
                body.push_str("\n[reasoning_snippet] ");
                body.push_str(&truncate_chars(r, MAX_REASONING_SNIPPET_CHARS));
            }
        }
        if let Some(tcs) = &m.tool_calls {
            for t in tcs {
                body.push_str(&format!(
                    "\n[tool {} args] {}",
                    t.name,
                    truncate_chars(&t.arguments, MAX_TOOL_ARGS_CHARS)
                ));
                if let Some(res) = &t.result {
                    body.push_str(&format!(
                        "\n[tool {} output] {}",
                        t.name,
                        truncate_chars(res, tool_output_snippet_limit(&t.name))
                    ));
                }
                if let Some(err) = &t.error {
                    body.push_str(&format!(
                        "\n[tool {} error] {}",
                        t.name,
                        truncate_chars(err, MAX_TOOL_ERROR_CHARS)
                    ));
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
            "{}\n\n… (prefix truncated to ~{} chars for summarization)",
            truncate_chars(&out, take),
            MAX_PREFIX_CHARS_FOR_API
        );
    }
    out
}

const SUMMARY_SYSTEM: &str = r#"You compress an OLDER prefix of a multi-turn agent session (user, assistant, tools).
The next message is that prefix excerpt. Tool lines use markers like [tool NAME args/output/error].

Output ONE structured summary. Use the section headings below in order.
Write section content in the same language the user mainly used (keep paths, commands, symbols, and errors literal).
If a section has nothing, write "(none)".

## Goals & constraints
## Decisions
## Code & files
## Commands & verification
## Tool evidence
## Sub-agent / explore handoffs
## Open issues & TODOs
## Unknown / truncated / not explicit in source

Retention rules (highest first):
1) User goals, hard constraints, and unfinished work
2) File paths with line ranges, symbols, and what was changed or planned
3) Shell/test/lint commands with pass/fail — never fabricate results
4) Errors and tool failures — quote or tightly paraphrase
5) run_subagent / explore conclusions and open questions
6) task_board status and validate contracts

Drop: repeated tool dumps, large file bodies, small talk, duplicate facts.
Never summarize tool output as "files were read" without naming paths and conclusions.

Never invent paths, line numbers, test outcomes, or config values.
If the excerpt was truncated, say so under Unknown.
Be dense; prefer bullets over prose."#;

fn build_summary_system_prompt(ui: &CompressionUiContext, keep_users: u32) -> String {
    let mut prompt = SUMMARY_SYSTEM.to_string();
    prompt.push_str(&format!(
        "\n\nHost context: the newest {keep_users} user turn(s) after this summary stay verbatim. \
         Summarize ONLY the older prefix; do not repeat facts likely still visible verbatim."
    ));
    match ui.scope {
        CompressionScope::SubAgent => {
            if ui.sub_agent_id.as_deref() == Some("explore") {
                prompt.push_str(
                    "\n\nSub-agent scope: read-only explore worker. Prioritize paths:lines, call chains, \
                     negative greps, and corrections to lead assumptions — not full file bodies.",
                );
            } else if ui.sub_agent_id.as_deref() == Some("research") {
                prompt.push_str(
                    "\n\nSub-agent scope: web-only research worker. Prioritize cited URLs, cross-checked \
                     external facts, and source-backed summaries — not codebase paths.",
                );
            } else if let Some(name) = ui.sub_agent_name.as_deref().filter(|s| !s.is_empty()) {
                prompt.push_str(&format!(
                    "\n\nSub-agent scope: {name}. Preserve handoff conclusions the lead agent will need."
                ));
            } else {
                prompt.push_str(
                    "\n\nSub-agent scope: isolated worker thread. Preserve conclusions needed for the lead handoff.",
                );
            }
        }
        CompressionScope::Main => {}
    }
    prompt
}

fn summary_fallback_notice() -> String {
    format!(
        "{SUMMARY_PREFIX_BUDGET}\n\n(Summary failed or was cancelled; older turns were dropped. Briefly restate your goal and critical context if you still need it.)"
    )
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
    ui: &CompressionUiContext,
) -> bool {
    let wall = Instant::now();
    let messages_before = history.len();
    if !settings.context_compression_enabled {
        log::info!(
            "context_compress: skip_disabled conversation_id={} messages={} wall_ms={}",
            conversation_id,
            messages_before,
            wall.elapsed().as_millis()
        );
        return false;
    }
    let keep_users = settings.context_keep_recent_user_turns.max(1);
    let budget = settings.context_budget_chars.max(4096);

    let est = estimate_message_payload_chars(history);
    if !force_ignore_char_budget && est <= budget as usize {
        log::info!(
            "context_compress: skip_under_budget conversation_id={} messages={} est_chars={} budget_chars={} wall_ms={}",
            conversation_id,
            messages_before,
            est,
            budget,
            wall.elapsed().as_millis()
        );
        return false;
    }

    let split = find_split_at_user_boundary(history, keep_users as usize);
    if split == 0 {
        log::info!(
            "context_compress: skip_no_user_boundary conversation_id={} messages={} est_chars={} wall_ms={}",
            conversation_id,
            messages_before,
            est,
            wall.elapsed().as_millis()
        );
        return false;
    }

    let prefix = &history[..split];
    if prefix.is_empty() {
        log::info!(
            "context_compress: skip_empty_prefix conversation_id={} messages={} wall_ms={}",
            conversation_id,
            messages_before,
            wall.elapsed().as_millis()
        );
        return false;
    }

    emit_ui_toast(
        stream,
        conversation_id,
        &compression_start_toast(ui),
        "warning",
    );

    let dropped_count = history[..split]
        .iter()
        .filter(|m| {
            crate::message_context::is_context_included(m)
                && !crate::message_context::is_synthetic_user_content(&m.content)
        })
        .count() as u32;
    let t_fmt = Instant::now();
    let formatted = format_prefix_for_summary(prefix);
    let format_prefix_ms = t_fmt.elapsed().as_millis();

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
            };

    let max_tok = settings.context_summary_max_tokens.max(128);
    let summary_prefix = if force_ignore_char_budget {
        SUMMARY_PREFIX_TOOL_LIMIT
    } else {
        SUMMARY_PREFIX_BUDGET
    };
    let reason = if force_ignore_char_budget {
        "tool_limit"
    } else {
        "budget"
    };
    let dump_lbl = format!(
        "{}_context_summary_{}",
        conversation_id,
        if force_ignore_char_budget {
            "tool_limit"
        } else {
            "budget"
        }
    );
    let t_llm = Instant::now();
    let mut summary_failed = false;
    let summary_system = build_summary_system_prompt(ui, keep_users);
    let summary_body = match provider
        .chat_once(
            std::slice::from_ref(&input),
            &crate::models::SystemPromptSections::all_cacheable(vec![summary_system]),
            Vec::new(),
            cancel.clone(),
            Some(max_tok),
            Some(dump_lbl.as_str()),
        )
        .await
    {
        Ok(out) => {
            let model = if provider.settings.model.trim().is_empty() {
                None
            } else {
                Some(provider.settings.model.as_str())
            };
            if let Some(scope) = ui.agent_scope.as_ref() {
                if let Err(e) =
                    crate::token_usage_store::record_round(scope, out.usage.as_ref(), model)
                {
                    log::warn!(
                        "token_usage_store: context compression record_round failed {}: {e}",
                        scope.log_suffix()
                    );
                }
            }
            let t = out.text.trim();
            let summary_llm_ms = t_llm.elapsed().as_millis();
            if t.is_empty() {
                summary_failed = true;
                log::warn!(
                    "context summary returned empty; using fallback notice (summary_llm_ms={summary_llm_ms})"
                );
                summary_fallback_notice()
            } else {
                format!("{summary_prefix}\n\n{t}")
            }
        }
        Err(e) => {
            summary_failed = true;
            let summary_llm_ms = t_llm.elapsed().as_millis();
            log::warn!(
                "context summary LLM call failed: {e}; using fallback notice (summary_llm_ms={summary_llm_ms})"
            );
            summary_fallback_notice()
        }
    };

    // 如果用户已取消，不要继续修改历史
    if cancel.is_cancelled() {
        log::info!(
            "context_compress: cancelled after LLM summary, aborting splice conversation_id={} wall_ms={}",
            conversation_id,
            wall.elapsed().as_millis()
        );
        return false;
    }

    let summary_msg = new_summary_user_message(summary_body);
    for m in history.iter_mut().take(split) {
        if crate::message_context::is_synthetic_user_content(&m.content) {
            continue;
        }
        if crate::message_context::is_context_included(m) {
            crate::message_context::mark_excluded(
                m,
                crate::models::ExcludedReason::ContextCompression,
            );
        }
    }
    history.insert(split, summary_msg);
    let messages_after = history.len();

    let compression = build_compression_info(
        ui,
        reason,
        messages_before,
        messages_after,
        dropped_count,
        keep_users,
    );

    log::info!(
        "context_compress: applied conversation_id={} scope={:?} reason={} messages_before={} messages_after={} split_at={} est_chars={} budget_chars={} format_prefix_ms={} summary_llm_ms={} wall_ms={}",
        conversation_id,
        ui.scope,
        reason,
        messages_before,
        messages_after,
        split,
        est,
        budget,
        format_prefix_ms,
        t_llm.elapsed().as_millis(),
        wall.elapsed().as_millis()
    );

    let (done_msg, done_level) =
        compression_done_toast(ui, dropped_count, keep_users, summary_failed);
    emit_ui_toast(stream, conversation_id, &done_msg, done_level);

    match ui.scope {
        CompressionScope::Main if emit_history_replaced => {
            let _ = stream.send(StreamEvent::HistoryReplaced {
                conversation_id: conversation_id.to_string(),
                messages: history.clone(),
                compression: Some(compression),
            });
        }
        CompressionScope::SubAgent => {
            if let Some(message_id) = ui.message_id.as_deref() {
                let _ = stream.send(StreamEvent::ContextCompressed {
                    conversation_id: conversation_id.to_string(),
                    message_id: message_id.to_string(),
                    compression,
                });
            } else {
                log::warn!(
                    "context_compress: sub_agent scope missing message_id conversation_id={}",
                    conversation_id
                );
            }
        }
        CompressionScope::Main => {}
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
    ui: CompressionUiContext,
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
        &ui,
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
    ui: CompressionUiContext,
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
        &ui,
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

    #[test]
    fn sub_agent_ui_context_carries_agent_fields() {
        let ui = CompressionUiContext::sub_agent(
            AgentInstanceScope::new("conv", "explore"),
            "msg_1",
            "explore",
            "Explore Agent",
            "task_a",
        );
        assert_eq!(ui.scope, CompressionScope::SubAgent);
        assert_eq!(ui.sub_agent_id.as_deref(), Some("explore"));
        assert_eq!(ui.task_id.as_deref(), Some("task_a"));
    }

    #[test]
    fn user_messages_get_larger_snippet_than_assistant() {
        assert!(content_snippet_limit(&Role::User) > content_snippet_limit(&Role::Assistant));
    }

    #[test]
    fn grep_tool_output_limit_exceeds_file_read() {
        assert!(tool_output_snippet_limit("file:grep") > tool_output_snippet_limit("file:read"));
    }

    #[test]
    fn summary_system_prompt_includes_keep_users_and_explore_hint() {
        let ui = CompressionUiContext::sub_agent(
            AgentInstanceScope::new("conv", "explore"),
            "m",
            "explore",
            "Explore Agent",
            "t",
        );
        let p = build_summary_system_prompt(&ui, 6);
        assert!(p.contains("## Goals & constraints"));
        assert!(p.contains("newest 6 user turn"));
        assert!(p.contains("read-only explore"));
    }
}
