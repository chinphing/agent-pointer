//! Soft-exclude conversation prefix after successful `task_board` updates (no LLM summarization).

use crate::context_compression::{
    SUMMARY_PREFIX_BUDGET, SUMMARY_PREFIX_TOOL_LIMIT,
};
use crate::message_context::{
    count_context_included_messages, find_split_at_user_boundary, is_context_included,
    mark_excluded,
};
use crate::models::{ChatMessage, ExcludedReason, ModelSettings, Role, StreamEvent};
use std::collections::HashMap;
use tokio::sync::mpsc::UnboundedSender;

/// Prefix on legacy placeholder user rows after task_board-driven trim (UI detects this for styling).
pub const TRIM_PLACEHOLDER_PREFIX: &str = "[History trimmed after task_board update]";

const DEFAULT_KEEP_LAST_N_USERS: usize = 2;
/// Do not trim when fewer than this many messages are still included in LLM context.
const MIN_INCLUDED_MESSAGES_FOR_TRIM: usize = 10;
/// Computer: keep task-board anchor user + last N messages (+ latest live `[CUR_SCREEN]` inject).
const COMPUTER_KEEP_LAST_MESSAGES: usize = 10;
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
    /// User message id this task board is bound to (`get_main_task_board_anchor` / store key).
    pub anchor_message_id: Option<&'a str>,
}

fn normalize_agent_id(agent_id: &str) -> String {
    let t = agent_id.trim();
    if t.is_empty() {
        crate::agents::DEFAULT_AGENT_ID.to_string()
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

fn find_index_by_message_id(msgs: &[ChatMessage], message_id: &str) -> Option<usize> {
    let id = message_id.trim();
    if id.is_empty() {
        return None;
    }
    msgs.iter().position(|m| m.id == id)
}

/// Resolve the user row to always keep: task-board anchor, else first real user task.
pub fn resolve_task_board_anchor_user_index(
    msgs: &[ChatMessage],
    anchor_message_id: Option<&str>,
) -> Option<usize> {
    if let Some(anchor_id) = anchor_message_id.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(idx) = find_index_by_message_id(msgs, anchor_id) {
            if matches!(msgs[idx].role, Role::User) {
                return Some(idx);
            }
            log::warn!(
                "task_board_trim: anchor_message_id={anchor_id} is not a user message; falling back"
            );
        } else {
            log::warn!(
                "task_board_trim: anchor_message_id={anchor_id} not found in history; falling back"
            );
        }
    }
    find_first_real_user_index(msgs)
}

fn mark_range_excluded(history: &mut [ChatMessage], start: usize, end: usize) {
    for m in history.iter_mut().take(end).skip(start) {
        if is_context_included(m) {
            mark_excluded(m, ExcludedReason::TaskBoardTrim);
        }
    }
}

/// Live `[CUR_SCREEN]` inject (not the stripped-history placeholder).
pub fn is_live_cur_screen_inject(m: &ChatMessage) -> bool {
    if !matches!(m.role, Role::User) {
        return false;
    }
    let t = m.content.trim_start();
    t.starts_with(CUR_SCREEN_TAG) && !t.starts_with(CUR_SCREEN_OMITTED)
}

fn find_latest_live_cur_screen_index(msgs: &[ChatMessage]) -> Option<usize> {
    msgs.iter()
        .enumerate()
        .rev()
        .find_map(|(i, m)| is_live_cur_screen_inject(m).then_some(i))
}

fn collect_keep_indices(
    msgs: &[ChatMessage],
    keep_last_messages: usize,
    anchor_message_id: Option<&str>,
) -> Vec<usize> {
    let len = msgs.len();
    if len == 0 {
        return Vec::new();
    }
    let mut keep = std::collections::HashSet::new();
    if let Some(i) = resolve_task_board_anchor_user_index(msgs, anchor_message_id) {
        keep.insert(i);
    }
    let tail_start = len.saturating_sub(keep_last_messages.max(1));
    for i in tail_start..len {
        keep.insert(i);
    }
    if let Some(i) = find_latest_live_cur_screen_index(msgs) {
        keep.insert(i);
    }
    let mut indices: Vec<usize> = keep.into_iter().collect();
    indices.sort_unstable();
    indices
}

/// Computer trim: task-board anchor user, last `keep_last_messages` rows, and latest `[CUR_SCREEN]` inject.
pub fn trim_history_first_user_and_tail(
    history: &mut [ChatMessage],
    keep_last_messages: usize,
    anchor_message_id: Option<&str>,
) -> Option<TaskBoardTrimStats> {
    let messages_before = history.len();
    if messages_before == 0 {
        return None;
    }
    let keep = collect_keep_indices(history, keep_last_messages, anchor_message_id);
    if keep.len() >= messages_before {
        return None;
    }
    let split_at = keep.first().copied().unwrap_or(0);
    let mut dropped_count = 0u32;
    for (i, m) in history.iter_mut().enumerate() {
        if keep.binary_search(&i).is_ok() {
            continue;
        }
        if is_context_included(m) {
            mark_excluded(m, ExcludedReason::TaskBoardTrim);
            dropped_count += 1;
        }
    }
    if dropped_count == 0 {
        return None;
    }
    Some(TaskBoardTrimStats {
        messages_before,
        messages_after: history.len(),
        split_at,
        dropped_count,
    })
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

    let included_count = count_context_included_messages(history);
    if included_count < MIN_INCLUDED_MESSAGES_FOR_TRIM {
        log::info!(
            "task_board_trim: skip_few_included conversation_id={} agent_id={} included={} min={}",
            hook.conversation_id,
            hook.agent_id,
            included_count,
            MIN_INCLUDED_MESSAGES_FOR_TRIM
        );
        return;
    }

    let agent = normalize_agent_id(hook.agent_id);
    let stats = if agent == "computer" {
        trim_history_first_user_and_tail(
            history,
            COMPUTER_KEEP_LAST_MESSAGES,
            hook.anchor_message_id,
        )
    } else {
        trim_history_after_task_board(history, DEFAULT_KEEP_LAST_N_USERS)
    };
    let Some(stats) = stats else {
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
        if let Ok(store) = crate::conversation_store::global_store() {
            if let Err(e) = store.sync_messages_ordered(hook.conversation_id, history) {
                log::warn!(
                    "conversation_store: sync after task_board trim failed conversation_id={}: {e:#}",
                    hook.conversation_id
                );
            }
        }
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
    m.insert("general".into(), false);
    m.insert("explore".into(), false);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u_with_id(id: &str, content: &str) -> ChatMessage {
        let mut m = u(content);
        m.id = id.to_string();
        m
    }

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
            tool_raw_output: None,
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
            tool_raw_output: None,
        }
    }

    fn is_excluded(m: &ChatMessage) -> bool {
        m.context_state
            .as_ref()
            .map(|s| !s.included)
            .unwrap_or(false)
    }

    #[test]
    fn computer_trim_keeps_first_user_last_ten_and_latest_cur_screen() {
        let mut hist = vec![u("original task")];
        for i in 0..14 {
            hist.push(a());
            hist.push(u(&format!("filler user {i}")));
        }
        let latest_screen_idx = hist.len();
        let mut screen = u("[CUR_SCREEN] live frame");
        screen.images_base64 = Some(vec!["img".into()]);
        hist.push(screen);
        hist.push(a());

        let before_len = hist.len();
        let stats = trim_history_first_user_and_tail(&mut hist, 10, None).expect("trim");
        assert!(stats.dropped_count > 0);
        assert_eq!(hist.len(), before_len);
        assert!(!is_excluded(&hist[0]));
        assert!(!is_excluded(&hist[latest_screen_idx]));
        assert!(hist.iter().any(is_excluded));
        let tail_start = before_len.saturating_sub(10);
        for i in tail_start..before_len {
            assert!(!is_excluded(&hist[i]), "tail index {i} should stay included");
        }
    }

    #[test]
    fn computer_trim_keeps_cur_screen_outside_tail_window() {
        let mut hist = vec![u("task"), a()];
        let mut screen = u("[CUR_SCREEN] mid inject");
        screen.images_base64 = Some(vec!["x".into()]);
        hist.push(screen.clone());
        for _ in 0..12 {
            hist.push(a());
            hist.push(u("noise"));
        }
        let screen_idx = 2;
        let stats = trim_history_first_user_and_tail(&mut hist, 10, None).expect("trim");
        assert!(stats.dropped_count > 0);
        assert!(!is_excluded(&hist[0]));
        assert!(!is_excluded(&hist[screen_idx]));
    }

    #[test]
    fn computer_trim_keeps_task_board_anchor_not_first_user() {
        let mut hist = vec![u_with_id("first-user", "older unrelated task")];
        for i in 0..8 {
            hist.push(a());
            hist.push(u(&format!("noise {i}")));
        }
        let anchor_idx = hist.len();
        hist.push(u_with_id("anchor-msg", "task bound to board"));
        for i in 0..6 {
            hist.push(a());
            hist.push(u(&format!("tail noise {i}")));
        }
        let stats =
            trim_history_first_user_and_tail(&mut hist, 10, Some("anchor-msg")).expect("trim");
        assert!(stats.dropped_count > 0);
        assert!(!is_excluded(&hist[anchor_idx]));
        assert!(
            is_excluded(&hist[0]),
            "first user in conversation should be excluded when anchor is later"
        );
    }

    #[test]
    fn resolve_anchor_index_prefers_binding_over_first_user() {
        let msgs = vec![
            u_with_id("u1", "first"),
            a(),
            u_with_id("anchor-msg", "bound"),
        ];
        assert_eq!(
            resolve_task_board_anchor_user_index(&msgs, Some("anchor-msg")),
            Some(2)
        );
        assert_eq!(resolve_task_board_anchor_user_index(&msgs, None), Some(0));
    }

    #[test]
    fn computer_trim_skips_when_all_fit() {
        let mut hist = vec![u("only task"), a()];
        assert!(trim_history_first_user_and_tail(&mut hist, 10, None).is_none());
    }

    #[test]
    fn legacy_trim_marks_prefix_by_user_boundary() {
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
    }

    #[test]
    fn legacy_trim_skips_when_few_users() {
        let mut hist = vec![u("only task"), a()];
        assert!(trim_history_after_task_board(&mut hist, 2).is_none());
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
        assert!(task_board_call_is_checkpoint("task_board_replace", &args));
    }

    #[test]
    fn patch_trim_in_progress_only_does_not_trigger() {
        let pending = serde_json::json!({
            "items": [{ "id": "1", "status": "in_progress" }]
        });
        assert!(!task_board_call_is_checkpoint("task_board_patch", &pending));
    }

    #[test]
    fn patch_trim_triggers_on_done_validate_results_or_progress() {
        let done = serde_json::json!({
            "items": [{ "id": "1", "status": "done" }]
        });
        assert!(task_board_call_is_checkpoint("task_board_patch", &done));
        let evidence = serde_json::json!({
            "items": [{
                "id": "1",
                "status": "in_progress",
                "validate_results": "微信: opened"
            }]
        });
        assert!(task_board_call_is_checkpoint("task_board_patch", &evidence));
        let progress = serde_json::json!({
            "items": [{ "id": "1", "progress": "3/10" }]
        });
        assert!(task_board_call_is_checkpoint("task_board_patch", &progress));
        let legacy_checkpoint = serde_json::json!({
            "items": [{ "id": "1", "checkpoint": "3/10" }]
        });
        assert!(task_board_call_is_checkpoint("task_board_patch", &legacy_checkpoint));
    }

    #[test]
    fn trim_skips_when_few_included_messages() {
        let mut hist = vec![u("task"), a()];
        let hook = TaskBoardTrimHook {
            settings: &ModelSettings::default(),
            agent_id: "computer",
            conversation_id: "c1",
            stream: &tokio::sync::mpsc::unbounded_channel().0,
            emit_history_replaced: false,
            anchor_message_id: None,
        };
        maybe_trim_after_tool_pass(&mut hist, &hook, true);
        assert!(!hist.iter().any(is_excluded));
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
