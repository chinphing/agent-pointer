//! Message-level UI bindings vs LLM context participation.

use crate::context_compression::{SUMMARY_PREFIX_BUDGET, SUMMARY_PREFIX_TOOL_LIMIT};
use crate::models::{ChatMessage, ExcludedReason, MessageContextState, ModelSettings, Role};
use crate::task_board::history_trim::TRIM_PLACEHOLDER_PREFIX;

/// Which transcript filter applies when building provider HTTP messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LlmHistoryScope {
    /// Lead agent loop: scoped sub-agent rows never enter parent LLM context.
    #[default]
    Lead,
    /// Sub-agent loop: full assistant + tool chain in `local_history`; ignore scoped linkage.
    SubAgentLoop,
}

/// Whether this row may enter its own thread's LLM call.
///
/// `included` defaults to true. `included=false` with no excluded reason is the
/// legacy scope stamp and still enters. A real reason (compression, trim,
/// planner shell) stays out. Thread membership is `agentInstanceId`, not this flag.
/// Lead wire additionally drops rows that carry `anchorMessageId`.
pub fn is_context_included(m: &ChatMessage) -> bool {
    match m.context_state.as_ref() {
        None => true,
        Some(state) if state.included => true,
        Some(state) => state.excluded_reason.is_none(),
    }
}

/// Sub-agent loop inclusion. Same admission rule as [`is_context_included`].
pub fn is_sub_agent_loop_included(m: &ChatMessage) -> bool {
    is_context_included(m)
}

/// Fill a missing in-memory `agentInstanceId` so the live loop matches the persisted thread.
pub fn ensure_loop_instance_id(history: &mut [ChatMessage], instance_id: &str) {
    let id = instance_id.trim();
    if id.is_empty() {
        log::warn!("message_context: skip instance stamp, empty agent_instance_id");
        return;
    }
    let mut filled = 0u32;
    for msg in history.iter_mut() {
        let missing = msg
            .agent_instance_id
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty();
        if missing {
            msg.agent_instance_id = Some(id.to_string());
            filled += 1;
        }
    }
    if filled > 0 {
        log::info!(
            "message_context: stamped in-memory instance count={filled} agent_instance_id={id}"
        );
    }
}

pub fn mark_excluded(m: &mut ChatMessage, reason: ExcludedReason) {
    m.context_state = Some(MessageContextState {
        included: false,
        excluded_reason: Some(reason),
    });
}

pub fn filter_context_messages(msgs: &[ChatMessage]) -> Vec<ChatMessage> {
    msgs.iter()
        .filter(|m| !crate::models::is_scoped_sub_message(m) && is_context_included(m))
        .cloned()
        .collect()
}

pub fn filter_sub_agent_loop_messages(msgs: &[ChatMessage]) -> Vec<ChatMessage> {
    msgs.iter()
        .filter(|m| is_sub_agent_loop_included(m))
        .cloned()
        .collect()
}

pub fn filter_messages_for_llm_scope(
    msgs: &[ChatMessage],
    scope: LlmHistoryScope,
) -> Vec<ChatMessage> {
    match scope {
        LlmHistoryScope::Lead => filter_context_messages(msgs),
        LlmHistoryScope::SubAgentLoop => filter_sub_agent_loop_messages(msgs),
    }
}

fn is_included_for_llm_scope(m: &ChatMessage, scope: LlmHistoryScope) -> bool {
    match scope {
        LlmHistoryScope::Lead => {
            !crate::models::is_scoped_sub_message(m) && is_context_included(m)
        }
        LlmHistoryScope::SubAgentLoop => is_sub_agent_loop_included(m),
    }
}

/// Count messages that still participate in LLM context (`included` default true).
pub fn count_context_included_messages(msgs: &[ChatMessage]) -> usize {
    msgs.iter().filter(|m| is_context_included(m)).count()
}

fn excluded_reason_tag(reason: &ExcludedReason) -> &'static str {
    match reason {
        ExcludedReason::ContextCompression => "context_compression",
        ExcludedReason::TaskBoardTrim => "task_board_trim",
        ExcludedReason::PlannerUiShell => "planner_ui_shell",
    }
}

fn excluded_message_log_line(m: &ChatMessage) -> String {
    let reason = m
        .context_state
        .as_ref()
        .and_then(|s| s.excluded_reason.as_ref())
        .map(excluded_reason_tag)
        .unwrap_or("unknown");
    let role = format!("{:?}", m.role).to_ascii_lowercase();
    let preview: String = m.content.chars().take(120).collect();
    format!(
        "  id={} role={} excludedReason={} included=false preview={preview:?}",
        m.id, role, reason
    )
}

/// When debug prompt dump is enabled, log messages omitted from LLM context (`included=false`).
pub fn try_log_context_excluded_messages(
    settings: &ModelSettings,
    msgs: &[ChatMessage],
    phase: &str,
    label: Option<&str>,
    scope: LlmHistoryScope,
) {
    if !crate::llm_prompt_dump::should_dump(settings) {
        return;
    }
    let lines: Vec<String> = msgs
        .iter()
        .filter(|m| !is_included_for_llm_scope(m, scope))
        .map(excluded_message_log_line)
        .collect();
    if lines.is_empty() {
        return;
    }
    log::info!(
        "context_excluded_messages phase={} label={} count={}\n{}",
        phase,
        label.unwrap_or("-"),
        lines.len(),
        lines.join("\n")
    );
}

/// User rows that are synthetic (compression summary, trim placeholder, screen inject).
pub fn is_synthetic_user_content(content: &str) -> bool {
    let t = content.trim_start();
    t.starts_with(SUMMARY_PREFIX_BUDGET)
        || t.starts_with(SUMMARY_PREFIX_TOOL_LIMIT)
        || t.starts_with(TRIM_PLACEHOLDER_PREFIX)
        || t.starts_with("你的上一次回复为空")
        || t.starts_with("【环境反馈】")
        || t.starts_with("【输出长度】")
}

/// User turns that depend entirely on prior task context should not consume one
/// of the limited verbatim-retention boundaries during compression.
pub fn is_context_dependent_user_content(content: &str) -> bool {
    matches!(
        content.trim().to_ascii_lowercase().as_str(),
        "继续"
            | "继续处理"
            | "继续执行"
            | "continue"
            | "continue."
            | "go on"
            | "go ahead"
            | "proceed"
    )
}

/// Whether this row is a real user turn (included, not a summary/placeholder).
pub fn is_real_context_user(m: &ChatMessage) -> bool {
    is_context_included(m)
        && matches!(m.role, Role::User)
        && !is_synthetic_user_content(&m.content)
        && !is_context_dependent_user_content(&m.content)
}

/// Newest-to-oldest scan, then reversed: last `n` real user indices, oldest first.
pub fn find_recent_context_user_indices(msgs: &[ChatMessage], n: usize) -> Vec<usize> {
    if n == 0 || msgs.is_empty() {
        return Vec::new();
    }
    let mut idx = Vec::new();
    for i in (0..msgs.len()).rev() {
        if is_real_context_user(&msgs[i]) {
            idx.push(i);
            if idx.len() == n {
                break;
            }
        }
    }
    idx.reverse();
    idx
}

/// Last included assistant in this user turn that concluded the turn
/// (no trailing `role: tool` rows, and any `tool_calls` are resolved).
pub fn concluding_assistant_index(msgs: &[ChatMessage], user_idx: usize) -> Option<usize> {
    if user_idx >= msgs.len() {
        return None;
    }
    let end = ((user_idx + 1)..msgs.len())
        .find(|&i| is_context_included(&msgs[i]) && matches!(msgs[i].role, Role::User))
        .unwrap_or(msgs.len());
    let mut last_asst = None;
    for i in (user_idx + 1)..end {
        if !is_context_included(&msgs[i]) {
            continue;
        }
        if matches!(msgs[i].role, Role::Assistant) {
            last_asst = Some(i);
        }
    }
    let i = last_asst?;
    let trailing_tool =
        ((i + 1)..end).any(|j| is_context_included(&msgs[j]) && matches!(msgs[j].role, Role::Tool));
    if trailing_tool {
        return None;
    }
    if !assistant_tool_calls_resolved(&msgs[i]) {
        return None;
    }
    Some(i)
}

fn assistant_tool_calls_resolved(m: &ChatMessage) -> bool {
    match m.tool_calls.as_ref() {
        None => true,
        Some(calls) if calls.is_empty() => true,
        Some(calls) => calls
            .iter()
            .all(|c| c.result.is_some() || c.error.as_ref().is_some_and(|e| !e.trim().is_empty())),
    }
}

/// Index of the newest context-included, non-synthetic user turn.
pub fn find_last_context_user_index(msgs: &[ChatMessage]) -> Option<usize> {
    find_recent_context_user_indices(msgs, 1)
        .into_iter()
        .next_back()
}

/// Start index of the Nth **context-included** user message from the end.
pub fn find_split_at_user_boundary(msgs: &[ChatMessage], keep_last_n_users: usize) -> usize {
    if keep_last_n_users == 0 || msgs.is_empty() {
        return 0;
    }
    let mut seen = 0usize;
    for i in (0..msgs.len()).rev() {
        if is_real_context_user(&msgs[i]) {
            seen += 1;
            if seen == keep_last_n_users {
                return i;
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ChatMessage;

    fn u(content: &str) -> ChatMessage {
        ChatMessage {
            id: format!("u_{}", uuid::Uuid::new_v4().simple()),
            role: Role::User,
            content: content.into(),
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
            tool_raw_output: None,
        }
    }

    #[test]
    fn filter_excludes_marked_messages() {
        let mut excluded = u("old");
        mark_excluded(&mut excluded, ExcludedReason::ContextCompression);
        let msgs = vec![excluded, u("recent")];
        let filtered = filter_context_messages(&msgs);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].content, "recent");
    }

    #[test]
    fn split_skips_excluded_users() {
        let mut old = u("old");
        mark_excluded(&mut old, ExcludedReason::TaskBoardTrim);
        let msgs = vec![old, u("a"), u("b"), u("c")];
        assert_eq!(find_split_at_user_boundary(&msgs, 1), 3);
        assert_eq!(find_split_at_user_boundary(&msgs, 2), 2);
    }

    #[test]
    fn split_skips_retry_and_context_dependent_user_turns() {
        let msgs = vec![
            u("original task"),
            u("你的上一次回复为空，必须重试"),
            u("继续"),
            u("follow-up requirement"),
        ];
        assert_eq!(find_split_at_user_boundary(&msgs, 1), 3);
        assert_eq!(find_split_at_user_boundary(&msgs, 2), 0);
        assert_eq!(find_last_context_user_index(&msgs), Some(3));
    }

    #[test]
    fn sub_agent_loop_keeps_stamped_assistant_tool_pair() {
        let mut assistant = u("plan");
        assistant.role = Role::Assistant;
        assistant.anchor_message_id = Some("lead_anchor".into());
        assistant.context_state = Some(MessageContextState {
            included: false,
            excluded_reason: None,
        });
        assistant.tool_calls = Some(vec![crate::models::ToolCall {
            id: "call_1".into(),
            name: "terminal".into(),
            arguments: "{}".into(),
            status: "success".into(),
            result: None,
            error: None,
            duration_ms: None,
            risk_level: None,
            display_label: None,
            display_summary: None,
        }]);
        let mut tool = u("ok");
        tool.role = Role::Tool;
        tool.tool_call_id = Some("call_1".into());
        let msgs = vec![assistant.clone(), tool.clone()];
        assert_eq!(
            filter_context_messages(&msgs).len(),
            1,
            "lead keeps orphan tool only"
        );
        let kept = filter_sub_agent_loop_messages(&msgs);
        assert_eq!(kept.len(), 2);
        assert_eq!(
            filter_messages_for_llm_scope(&msgs, LlmHistoryScope::Lead).len(),
            1
        );
        assert_eq!(
            filter_messages_for_llm_scope(&msgs, LlmHistoryScope::SubAgentLoop).len(),
            2
        );
    }

    #[test]
    fn sub_agent_loop_still_honors_compression_exclusion() {
        let mut excluded = u("old");
        excluded.anchor_message_id = Some("lead_anchor".into());
        mark_excluded(&mut excluded, ExcludedReason::ContextCompression);
        let kept = filter_sub_agent_loop_messages(&[excluded]);
        assert!(kept.is_empty());
    }
}
