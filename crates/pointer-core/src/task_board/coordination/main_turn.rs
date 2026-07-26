//! Main-agent task board keying bound to user turns.

use crate::message_context::is_synthetic_user_content;
use crate::models::{ChatMessage, Role};

pub const MAIN_TURN_KEY_SEP: &str = "\u{1f}ptr_main_turn\u{1f}";

pub fn main_turn_task_board_store_key(conversation_id: &str, user_message_id: &str) -> String {
    format!(
        "{conv}{MAIN_TURN_KEY_SEP}{msg}",
        conv = conversation_id.trim(),
        msg = user_message_id.trim()
    )
}

pub fn is_main_turn_store_key(store_key: &str) -> bool {
    store_key.contains(MAIN_TURN_KEY_SEP)
}

pub fn conversation_id_from_main_turn_key(store_key: &str) -> Option<String> {
    let (conv, _) = store_key.split_once(MAIN_TURN_KEY_SEP)?;
    let conv = conv.trim();
    if conv.is_empty() {
        None
    } else {
        Some(conv.to_string())
    }
}

pub fn anchor_message_id_from_main_turn_key(store_key: &str) -> Option<String> {
    let (_, msg) = store_key.split_once(MAIN_TURN_KEY_SEP)?;
    let msg = msg.trim();
    if msg.is_empty() {
        None
    } else {
        Some(msg.to_string())
    }
}

pub fn looks_like_resume_intent(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    if t.is_empty() {
        return false;
    }
    let cn_keywords = [
        "继续",
        "接着",
        "接下来",
        "上次",
        "之前",
        "刚才",
        "延续",
        "补完",
        "继续执行",
        "继续做",
    ];
    if cn_keywords.iter().any(|k| t.contains(k)) {
        return true;
    }
    let en_keywords = [
        "continue",
        "resume",
        "pick up",
        "follow up",
        "carry on",
        "as above",
        "previous task",
    ];
    en_keywords.iter().any(|k| t.contains(k))
}

/// Latest non-synthetic **lead** user message id in transcript order
/// (for main-turn board binding and turn-file baselines).
///
/// Scoped sub-agent rows (host stub / nested user hints) are ignored so a
/// delegated `file_edit` / `file_write` still anchors to the real user turn.
pub fn latest_real_user_message_id(history: &[ChatMessage]) -> Option<String> {
    history.iter().rev().find_map(|m| {
        if !matches!(m.role, Role::User)
            || is_synthetic_user_content(&m.content)
            || crate::models::is_scoped_sub_message(m)
        {
            return None;
        }
        let id = m.id.trim();
        if id.is_empty() {
            None
        } else {
            Some(id.to_string())
        }
    })
}

/// When `init` runs while `current_store_key` already has board content, the model chose a
/// **new** scope — host opens a fresh board on the current user-turn key.
pub fn fresh_main_turn_store_key_for_init(
    conversation_id: &str,
    current_store_key: &str,
    user_message_id: &str,
) -> Option<String> {
    let fresh = main_turn_task_board_store_key(conversation_id, user_message_id);
    if fresh == current_store_key.trim() {
        None
    } else {
        Some(fresh)
    }
}

/// When execution calls `init` on a non-empty main-turn board, bind init to a fresh user-turn key.
pub fn resolve_fresh_main_turn_init_store_key(
    store: &crate::task_board::TaskBoardStore,
    conversation_id: &str,
    current_store_key: &str,
    history: &[ChatMessage],
) -> Option<String> {
    if crate::task_board::is_child_store_key(current_store_key) {
        return None;
    }
    if store.document(current_store_key).board_is_empty() {
        return None;
    }
    let uid = latest_real_user_message_id(history)?;
    fresh_main_turn_store_key_for_init(conversation_id, current_store_key, &uid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_roundtrip_anchor() {
        let key = main_turn_task_board_store_key("conv-1", "msg-1");
        assert!(is_main_turn_store_key(&key));
        assert_eq!(
            conversation_id_from_main_turn_key(&key).as_deref(),
            Some("conv-1")
        );
        assert_eq!(
            anchor_message_id_from_main_turn_key(&key).as_deref(),
            Some("msg-1")
        );
    }

    #[test]
    fn resume_intent_detects_cn_and_en() {
        assert!(looks_like_resume_intent("继续上次任务"));
        assert!(looks_like_resume_intent("please continue this task"));
        assert!(!looks_like_resume_intent("新建一个独立任务"));
    }

    #[test]
    fn latest_real_user_skips_scoped_sub_agent_stub() {
        let lead = ChatMessage {
            id: "user-lead".into(),
            role: Role::User,
            content: "please edit".into(),
            status: "done".into(),
            created_at: 1,
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
            attachments: None,
            anchor_message_id: None,
            trace_id: None,
            task_id: None,
            spawn_depth: None,
        };
        let mut stub = lead.clone();
        stub.id = "sub_task_stub".into();
        stub.content =
            "Begin. Your assigned task is in the system prompt under **Assigned task**.".into();
        stub.created_at = 2;
        stub.anchor_message_id = Some("assistant-anchor".into());
        stub.trace_id = Some("trace-1".into());

        assert_eq!(
            latest_real_user_message_id(&[lead, stub]).as_deref(),
            Some("user-lead")
        );
    }
}
