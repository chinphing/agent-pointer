//! Soft-exclude conversation prefix after successful `task_board` updates (no LLM summarization).

use crate::context_compression::{
    SUMMARY_PREFIX_BUDGET, SUMMARY_PREFIX_TOOL_LIMIT,
};
use crate::message_context::{find_split_at_user_boundary, is_context_included, mark_excluded};
use crate::models::{ChatMessage, ExcludedReason, ModelSettings, Role, StreamEvent};
use std::collections::HashMap;
use tokio::sync::mpsc::UnboundedSender;

/// Prefix on legacy placeholder user rows after task_board-driven trim (UI detects this for styling).
pub const TRIM_PLACEHOLDER_PREFIX: &str = "[History trimmed after task_board update]";

const DEFAULT_KEEP_LAST_N_USERS: usize = 2;
const CUR_SCREEN_TAG: &str = "[CUR_SCREEN]";
const CUR_SCREEN_OMITTED: &str =
    "[CUR_SCREEN] Earlier desktop screenshots are omitted here; use only the latest [CUR_SCREEN] message in this request for images.";

type StreamTx = UnboundedSender<StreamEvent>;

#[derive(Debug, Clone)]
pub struct TaskBoardTrimStats {
    pub messages_before: usize,
    pub messages_after: usize,
    pub split_at: usize,
    pub dropped_count: u32,
}

pub struct TaskBoardTrimHook<'a> {
    pub settings: &'a ModelSettings,
    pub agent_id: &'a str,
    pub conversation_id: &'a str,
    pub stream: &'a StreamTx,
    /// When true, emit `HistoryReplaced` so the chat UI persists updated flags.
    pub emit_history_replaced: bool,
}

fn normalize_agent_id(agent_id: &str) -> String {
    let t = agent_id.trim();
    if t.is_empty() {
        "default".into()
    } else {
        t.to_string()
    }
}

fn default_trim_for_agent(agent_id: &str) -> bool {
    agent_id == "computer"
}

/// Whether task_board stage trim is enabled for this worker id (settings override, then built-in default).
pub fn is_task_board_history_trim_enabled(settings: &ModelSettings, agent_id: &str) -> bool {
    let id = normalize_agent_id(agent_id);
    settings
        .agent_task_board_history_trim
        .get(&id)
        .copied()
        .unwrap_or_else(|| default_trim_for_agent(&id))
}

pub use crate::task_board::checkpoint::{is_task_board_tool_name, task_board_call_is_checkpoint};

/// User rows that are not the session's original task (injected screens, compression, trim placeholders).
pub fn is_injected_or_synthetic_user_content(content: &str) -> bool {
    let t = content.trim_start();
    t.starts_with(CUR_SCREEN_TAG)
        || t.starts_with(CUR_SCREEN_OMITTED)
        || t.starts_with(SUMMARY_PREFIX_BUDGET)
        || t.starts_with(SUMMARY_PREFIX_TOOL_LIMIT)
        || t.starts_with(TRIM_PLACEHOLDER_PREFIX)
}

pub fn is_real_user_task_message(m: &ChatMessage) -> bool {
    matches!(m.role, Role::User) && !is_injected_or_synthetic_user_content(&m.content)
}

fn find_first_real_user_index(msgs: &[ChatMessage]) -> Option<usize> {
    msgs.iter()
        .position(|m| is_real_user_task_message(m))
}

fn mark_range_excluded(history: &mut [ChatMessage], start: usize, end: usize) {
    for m in history.iter_mut().take(end).skip(start) {
        if is_context_included(m) {
            mark_excluded(m, ExcludedReason::TaskBoardTrim);
        }
    }
}

/// Soft-exclude prefix in `history` per task_board checkpoint policy. Returns `None` when no trim applied.
pub fn trim_history_after_task_board(
    history: &mut Vec<ChatMessage>,
    keep_last_n_users: usize,
) -> Option<TaskBoardTrimStats> {
    let keep = keep_last_n_users.max(1);
    let split = find_split_at_user_boundary(history, keep);
    if split == 0 {
        return None;
    }
    let first_idx = find_first_real_user_index(history)?;
    let messages_before = history.len();
    let first_in_suffix = split <= first_idx;

    let dropped_count = if !first_in_suffix {
        if split <= first_idx + 1 {
            return None;
        }
        let dropped_count = history[first_idx + 1..split]
            .iter()
            .filter(|m| is_context_included(m))
            .count() as u32;
        if dropped_count == 0 {
            return None;
        }
        mark_range_excluded(history, 0, first_idx);
        mark_range_excluded(history, first_idx + 1, split);
        dropped_count
    } else {
        let dropped_count = history[..split]
            .iter()
            .filter(|m| is_context_included(m))
            .count() as u32;
        if dropped_count == 0 {
            return None;
        }
        mark_range_excluded(history, 0, split);
        dropped_count
    };

    let messages_after = history.len();

    Some(TaskBoardTrimStats {
        messages_before,
        messages_after,
        split_at: split,
        dropped_count,
    })
}

pub fn maybe_trim_after_tool_pass(
    history: &mut Vec<ChatMessage>,
    hook: &TaskBoardTrimHook<'_>,
    task_board_succeeded: bool,
) {
    if !task_board_succeeded {
        return;
    }
    if !is_task_board_history_trim_enabled(hook.settings, hook.agent_id) {
        log::info!(
            "task_board_trim: skip_disabled conversation_id={} agent_id={}",
            hook.conversation_id,
            hook.agent_id
        );
        return;
    }

    let Some(stats) = trim_history_after_task_board(history, DEFAULT_KEEP_LAST_N_USERS) else {
        log::info!(
            "task_board_trim: skip_no_boundary conversation_id={} agent_id={} messages={}",
            hook.conversation_id,
            hook.agent_id,
            history.len()
        );
        return;
    };

    log::info!(
        "task_board_trim: applied conversation_id={} agent_id={} messages_before={} messages_after={} split_at={} excluded={}",
        hook.conversation_id,
        hook.agent_id,
        stats.messages_before,
        stats.messages_after,
        stats.split_at,
        stats.dropped_count
    );

    let toast = format!(
        "任务板更新后已将较早 {} 条对话从上下文排除",
        stats.dropped_count
    );
    let _ = hook.stream.send(StreamEvent::UiToast {
        conversation_id: hook.conversation_id.to_string(),
        message: toast,
        level: "info".to_string(),
    });

    if hook.emit_history_replaced {
        let _ = hook.stream.send(StreamEvent::HistoryReplaced {
            conversation_id: hook.conversation_id.to_string(),
            messages: history.clone(),
            compression: None,
        });
    }
}

/// Built-in defaults when the user has not set per-agent overrides (for UI hints).
pub fn default_agent_task_board_history_trim_table() -> HashMap<String, bool> {
    let mut m = HashMap::new();
    m.insert("computer".into(), true);
    m.insert("coder".into(), false);
    m.insert("default".into(), false);
    m.insert("explore".into(), false);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn a() -> ChatMessage {
        ChatMessage {
            id: format!("a_{}", uuid::Uuid::new_v4().simple()),
            role: Role::Assistant,
            content: "ok".into(),
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

    fn is_excluded(m: &ChatMessage) -> bool {
        m.context_state
            .as_ref()
            .map(|s| !s.included)
            .unwrap_or(false)
    }

    #[test]
    fn trim_marks_prefix_excluded_keeps_first_task_and_suffix() {
        let mut hist = vec![
            u("build the app"),
            a(),
            u("[CUR_SCREEN] screen"),
            a(),
            u("follow up"),
            a(),
        ];
        let before_len = hist.len();
        let stats = trim_history_after_task_board(&mut hist, 2).expect("trim");
        assert!(stats.dropped_count > 0);
        assert_eq!(hist.len(), before_len);
        assert!(!is_excluded(&hist[0]));
        assert!(hist.iter().any(|m| m.content.contains("follow up")));
        assert!(hist.iter().any(is_excluded));
    }

    #[test]
    fn trim_skips_when_few_users() {
        let mut hist = vec![u("only task"), a()];
        assert!(trim_history_after_task_board(&mut hist, 2).is_none());
        assert_eq!(hist.len(), 2);
    }

    #[test]
    fn enabled_defaults_computer_only() {
        let settings = ModelSettings::default();
        assert!(is_task_board_history_trim_enabled(&settings, "computer"));
        assert!(!is_task_board_history_trim_enabled(&settings, "coder"));
    }

    #[test]
    fn checkpoint_replace_always() {
        let args = serde_json::json!({ "items": [{ "id": "1", "status": "pending" }] });
        assert!(task_board_call_is_checkpoint("task_board:replace", &args));
    }

    #[test]
    fn checkpoint_patch_only_when_done() {
        let pending = serde_json::json!({
            "items": [{ "id": "1", "status": "in_progress" }]
        });
        assert!(!task_board_call_is_checkpoint("task_board:patch", &pending));
        let done = serde_json::json!({
            "items": [{ "id": "1", "status": "done" }]
        });
        assert!(task_board_call_is_checkpoint("task_board:patch", &done));
    }

    #[test]
    fn settings_override_wins() {
        let mut settings = ModelSettings::default();
        settings
            .agent_task_board_history_trim
            .insert("coder".into(), true);
        assert!(is_task_board_history_trim_enabled(&settings, "coder"));
    }
}
