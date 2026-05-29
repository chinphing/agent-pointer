//! Message-level UI bindings vs LLM context participation.

use crate::context_compression::{SUMMARY_PREFIX_BUDGET, SUMMARY_PREFIX_TOOL_LIMIT};
use crate::models::{ChatMessage, ExcludedReason, MessageContextState, Role};
use crate::task_board::history_trim::TRIM_PLACEHOLDER_PREFIX;

/// Whether this message participates in LLM context (default true when unset).
pub fn is_context_included(m: &ChatMessage) -> bool {
    m.context_state
        .as_ref()
        .map(|s| s.included)
        .unwrap_or(true)
}

pub fn mark_excluded(m: &mut ChatMessage, reason: ExcludedReason) {
    m.context_state = Some(MessageContextState {
        included: false,
        excluded_reason: Some(reason),
    });
}

pub fn filter_context_messages(msgs: &[ChatMessage]) -> Vec<ChatMessage> {
    msgs.iter()
        .filter(|m| is_context_included(m))
        .cloned()
        .collect()
}

/// User rows that are synthetic (compression summary, trim placeholder, screen inject).
pub fn is_synthetic_user_content(content: &str) -> bool {
    let t = content.trim_start();
    t.starts_with(SUMMARY_PREFIX_BUDGET)
        || t.starts_with(SUMMARY_PREFIX_TOOL_LIMIT)
        || t.starts_with(TRIM_PLACEHOLDER_PREFIX)
}

/// Start index of the Nth **context-included** user message from the end.
pub fn find_split_at_user_boundary(msgs: &[ChatMessage], keep_last_n_users: usize) -> usize {
    if keep_last_n_users == 0 || msgs.is_empty() {
        return 0;
    }
    let mut seen = 0usize;
    for i in (0..msgs.len()).rev() {
        if !is_context_included(&msgs[i]) {
            continue;
        }
        if matches!(msgs[i].role, Role::User) && !is_synthetic_user_content(&msgs[i].content) {
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
}
