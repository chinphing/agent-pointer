//! Incremental write paths (P0 append, P1 meta, P2 sync/replace).

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{ChatMessage, Conversation, ConversationMeta};

use super::persist::{
    conversation_preview, load_messages, message_index_content, role_str, upsert_conversation,
};

pub fn upsert_conversation_meta(conn: &Connection, meta: &ConversationMeta) -> Result<()> {
    let skill_ids_json = serde_json::to_string(&meta.skill_ids)?;
    let preview: Option<String> = conn
        .query_row(
            "SELECT preview FROM conversations WHERE id = ?1",
            params![meta.id],
            |row| row.get(0),
        )
        .optional()?;
    let preview = preview.unwrap_or_default();
    conn.execute(
        "INSERT INTO conversations (
           id, title, created_at_ms, updated_at_ms, message_count, preview,
           skill_ids_json, tool_rounds_used, tool_rounds_used_supervisor,
           computer_monitor_id, workspace_root
         ) VALUES (?1,?2,?3,?4,
           COALESCE((SELECT message_count FROM conversations WHERE id = ?1), 0),
           ?5,?6,?7,?8,?9,?10)
         ON CONFLICT(id) DO UPDATE SET
           title = excluded.title,
           updated_at_ms = excluded.updated_at_ms,
           skill_ids_json = excluded.skill_ids_json,
           tool_rounds_used = excluded.tool_rounds_used,
           tool_rounds_used_supervisor = excluded.tool_rounds_used_supervisor,
           computer_monitor_id = excluded.computer_monitor_id,
           workspace_root = excluded.workspace_root",
        params![
            meta.id,
            meta.title,
            meta.created_at,
            meta.updated_at,
            preview,
            skill_ids_json,
            meta.tool_rounds_used,
            meta.tool_rounds_used_supervisor,
            meta.computer_monitor_id,
            meta.workspace_root,
        ],
    )?;
    Ok(())
}

pub fn save_meta_all_in_conn(conn: &Connection, metas: &[ConversationMeta]) -> Result<()> {
    let ids: Vec<String> = metas.iter().map(|m| m.id.clone()).collect();
    super::persist::delete_conversations_not_in(conn, &ids)?;
    for meta in metas {
        upsert_conversation_meta(conn, meta)?;
    }
    Ok(())
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn ensure_conversation_row(conn: &Connection, conversation_id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM conversations WHERE id = ?1 LIMIT 1",
            params![conversation_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if exists {
        return Ok(());
    }
    let now = now_ms();
    upsert_conversation_meta(
        conn,
        &ConversationMeta {
            id: conversation_id.to_string(),
            title: "新会话".into(),
            created_at: now,
            updated_at: now,
            skill_ids: vec![],
            tool_rounds_used: 0,
            tool_rounds_used_supervisor: 0,
            computer_monitor_id: None,
            workspace_root: String::new(),
        },
    )
}

fn existing_message_ids(conn: &Connection, conversation_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT message_id FROM messages WHERE conversation_id = ?1 ORDER BY position ASC",
    )?;
    let rows = stmt.query_map(params![conversation_id], |row| row.get(0))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn max_message_position(conn: &Connection, conversation_id: &str) -> Result<i64> {
    let pos: Option<i64> = conn
        .query_row(
            "SELECT MAX(position) FROM messages WHERE conversation_id = ?1",
            params![conversation_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    Ok(pos.unwrap_or(-1))
}

fn insert_message_at(
    conn: &Connection,
    conversation_id: &str,
    msg: &ChatMessage,
    position: i64,
) -> Result<()> {
    let content = message_index_content(msg);
    let payload = serde_json::to_string(msg)?;
    conn.execute(
        "INSERT INTO messages (
           conversation_id, message_id, role, content, payload, created_at_ms, position
         ) VALUES (?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(conversation_id, message_id) DO UPDATE SET
           role = excluded.role,
           content = excluded.content,
           payload = excluded.payload,
           created_at_ms = excluded.created_at_ms,
           position = excluded.position",
        params![
            conversation_id,
            msg.id,
            role_str(&msg.role),
            content,
            payload,
            msg.created_at,
            position,
        ],
    )?;
    Ok(())
}

fn refresh_conversation_stats(conn: &Connection, conversation_id: &str) -> Result<()> {
    let messages = load_messages(conn, conversation_id)?;
    let preview = conversation_preview(&messages);
    conn.execute(
        "UPDATE conversations SET message_count = ?2, preview = ?3 WHERE id = ?1",
        params![conversation_id, messages.len() as i64, preview],
    )?;
    Ok(())
}

/// P0: append messages whose ids are not yet in the DB.
pub fn append_missing_messages_in_conn(
    conn: &Connection,
    conversation_id: &str,
    messages: &[ChatMessage],
) -> Result<u32> {
    ensure_conversation_row(conn, conversation_id)?;
    let existing: std::collections::HashSet<String> =
        existing_message_ids(conn, conversation_id)?.into_iter().collect();
    let mut pos = max_message_position(conn, conversation_id)?;
    let mut written = 0u32;
    for msg in messages {
        if existing.contains(&msg.id) {
            continue;
        }
        pos += 1;
        insert_message_at(conn, conversation_id, msg, pos)?;
        written += 1;
    }
    if written > 0 {
        refresh_conversation_stats(conn, conversation_id)?;
        log::debug!(
            "conversation_store: append_missing conversation_id={conversation_id} new_messages={written}"
        );
    }
    Ok(written)
}

/// P0: upsert a single message at the end (or update payload in place).
pub fn upsert_message_in_conn(
    conn: &Connection,
    conversation_id: &str,
    msg: &ChatMessage,
) -> Result<()> {
    ensure_conversation_row(conn, conversation_id)?;
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM messages WHERE conversation_id = ?1 AND message_id = ?2 LIMIT 1",
            params![conversation_id, msg.id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    let position = if exists {
        conn.query_row(
            "SELECT position FROM messages WHERE conversation_id = ?1 AND message_id = ?2",
            params![conversation_id, msg.id],
            |row| row.get::<_, i64>(0),
        )?
    } else {
        max_message_position(conn, conversation_id)? + 1
    };
    insert_message_at(conn, conversation_id, msg, position)?;
    refresh_conversation_stats(conn, conversation_id)?;
    Ok(())
}

/// P2a: upsert all messages in order; do not delete rows missing from the slice.
pub fn sync_messages_ordered_in_conn(
    conn: &Connection,
    conversation_id: &str,
    messages: &[ChatMessage],
) -> Result<()> {
    for (pos, msg) in messages.iter().enumerate() {
        insert_message_at(conn, conversation_id, msg, pos as i64)?;
    }
    refresh_conversation_stats(conn, conversation_id)?;
    log::info!(
        "conversation_store: sync_messages_ordered conversation_id={conversation_id} count={}",
        messages.len()
    );
    Ok(())
}

/// P2b: replace the full transcript for one conversation.
pub fn replace_messages_in_conn(
    conn: &Connection,
    conversation_id: &str,
    messages: &[ChatMessage],
) -> Result<()> {
    conn.execute(
        "DELETE FROM messages WHERE conversation_id = ?1",
        params![conversation_id],
    )?;
    for (pos, msg) in messages.iter().enumerate() {
        insert_message_at(conn, conversation_id, msg, pos as i64)?;
    }
    refresh_conversation_stats(conn, conversation_id)?;
    log::info!(
        "conversation_store: replace_messages conversation_id={conversation_id} count={}",
        messages.len()
    );
    Ok(())
}

/// Full save for tests / legacy import.
pub fn replace_all_in_conn(conn: &Connection, list: &[Conversation]) -> Result<()> {
    for conv in list {
        upsert_conversation(conn, conv, true)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_store::persist::sample_conv;
    use crate::conversation_store::ConversationStore;
    use crate::models::{ExcludedReason, MessageContextState, Role};
    use tempfile::TempDir;

    #[test]
    fn meta_save_does_not_wipe_messages() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let conv = sample_conv("c1", "T", "hello");
        store.save_all(&[conv.clone()]).unwrap();
        let meta = ConversationMeta {
            id: "c1".into(),
            title: "Renamed".into(),
            created_at: conv.created_at,
            updated_at: conv.updated_at + 1,
            skill_ids: vec![],
            tool_rounds_used: 2,
            tool_rounds_used_supervisor: 0,
            computer_monitor_id: None,
            workspace_root: "/tmp".into(),
        };
        store.save_meta_all(&[meta]).unwrap();
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded[0].messages.len(), 2);
        assert_eq!(loaded[0].messages[0].content, "hello");
        assert_eq!(loaded[0].title, "Renamed");
    }

    #[test]
    fn sync_preserves_messages_and_updates_context() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("c1", "T", "hello");
        store.save_all(&[conv.clone()]).unwrap();
        conv.messages[0].context_state = Some(MessageContextState {
            included: false,
            excluded_reason: Some(ExcludedReason::ContextCompression),
        });
        let summary = super::super::persist::msg(
            "sum1",
            Role::User,
            "[Conversation summary (auto-compression)] x",
            999,
        );
        conv.messages.insert(1, summary);
        store.sync_messages_ordered("c1", &conv.messages).unwrap();
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded[0].messages.len(), 3);
        assert_eq!(
            loaded[0].messages[0].context_state.as_ref().map(|s| s.included),
            Some(false)
        );
    }

    #[test]
    fn replace_trims_messages() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let conv = sample_conv("c1", "T", "hello");
        store.save_all(&[conv.clone()]).unwrap();
        store
            .replace_messages("c1", &[conv.messages[0].clone()])
            .unwrap();
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded[0].messages.len(), 1);
    }
}
