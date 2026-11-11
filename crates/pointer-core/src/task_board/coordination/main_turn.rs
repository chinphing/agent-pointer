//! Main-agent task board keying bound to user turns.

use crate::message_context::is_synthetic_user_content;
use crate::models::{ChatMessage, Role};
use crate::task_board::{BoardDocument, TaskBoardStore};
use anyhow::Result;

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
    store: &TaskBoardStore,
    conversation_id: &str,
    current_store_key: &str,
    history: &[ChatMessage],
) -> Option<String> {
    if crate::task_board::is_child_store_key(current_store_key) {
        log::debug!(
            "task_board_init: no fresh key, child scope store_key={current_store_key}"
        );
        return None;
    }
    if store.document(current_store_key).board_is_empty() {
        // First init of a turn: the key the host already opened is the right one.
        log::debug!("task_board_init: no fresh key, current board is empty store_key={current_store_key}");
        return None;
    }
    let Some(uid) = latest_real_user_message_id(history) else {
        // No real user turn in history (sub-agent / synthetic-only). Splitting
        // here would anchor the new board to nothing.
        log::debug!(
            "task_board_init: no fresh key, no real user message in history store_key={current_store_key}"
        );
        return None;
    };
    let fresh = fresh_main_turn_store_key_for_init(conversation_id, current_store_key, &uid);
    if fresh.is_none() {
        log::debug!(
            "task_board_init: no fresh key, current key already belongs to user message {uid} store_key={current_store_key}"
        );
    }
    fresh
}

/// Mark the previous unfinished parent board failed before a fresh `task_board_init`.
///
/// Without this, resume-rebound boards stay `running` next to the new active board
/// and the UI shows two stacked taskboards for the same turn.
pub fn abandon_previous_board_for_fresh_init(
    store: &TaskBoardStore,
    previous_store_key: &str,
) -> Result<BoardDocument> {
    let key = previous_store_key.trim();
    if key.is_empty() {
        anyhow::bail!("task_board: empty previous store key for fresh-init abandon");
    }
    store.apply(key, "abandon", &serde_json::json!({}))?;
    Ok(store.document(key))
}

/// Prefer the store-key-embedded originating user message after superseding a board,
/// so the abandoned board returns to its original anchor instead of stacking on the
/// current turn (where resume may have rebound it).
pub fn supersede_anchor_for_previous_board(previous_store_key: &str) -> Option<String> {
    anchor_message_id_from_main_turn_key(previous_store_key)
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
    fn fresh_init_key_needs_content_and_a_different_anchor() {
        let store = TaskBoardStore::new();
        let conv = "conv-fresh";
        let old = main_turn_task_board_store_key(conv, "msg-old");
        let user = |id: &str| {
            let mut m = crate::models::ChatMessage::user_text("do it");
            m.id = id.to_string();
            m
        };

        // Placeholder key, nothing written yet → the key already open is right.
        assert_eq!(
            resolve_fresh_main_turn_init_store_key(&store, conv, &old, &[user("msg-new")]),
            None
        );

        store
            .apply(
                &old,
                "init",
                &serde_json::json!({
                    "goal": "migrate",
                    "items": [{"id": "m1", "title": "A", "status": "pending"}]
                }),
            )
            .expect("init board");

        // Non-empty board + a newer user turn → init opens a fresh key on it.
        assert_eq!(
            resolve_fresh_main_turn_init_store_key(&store, conv, &old, &[user("msg-new")]),
            Some(main_turn_task_board_store_key(conv, "msg-new"))
        );
        // Same anchor as the open board → keep it.
        assert_eq!(
            resolve_fresh_main_turn_init_store_key(&store, conv, &old, &[user("msg-old")]),
            None
        );
    }

    #[test]
    fn abandon_previous_board_for_fresh_init_marks_failed() {
        use crate::task_board::{BoardItem, ItemStatus, MetaStatus};

        let store = TaskBoardStore::new();
        let key = main_turn_task_board_store_key("conv-1", "msg-old");
        let mut doc = BoardDocument::empty_for_store_key(&key);
        doc.meta.goal = "old goal".into();
        doc.meta.status = MetaStatus::Running;
        doc.global_milestones.push(BoardItem {
            id: "g1".into(),
            title: "step".into(),
            status: ItemStatus::InProgress,
            ..Default::default()
        });
        store.save_document(&key, doc);

        let out = abandon_previous_board_for_fresh_init(&store, &key).expect("abandon");
        assert!(matches!(out.meta.status, MetaStatus::Failed));
        assert_eq!(out.global_milestones[0].status, ItemStatus::Cancelled);
        assert_eq!(
            supersede_anchor_for_previous_board(&key).as_deref(),
            Some("msg-old")
        );
    }

    #[test]
    fn latest_real_user_skips_scoped_sub_agent_stub() {
        let lead = ChatMessage {
            turn_elapsed_ms: None,
            id: "user-lead".into(),
            role: Role::User,
            content: "please edit".into(),
            status: "done".into(),
            created_at: 1,
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
