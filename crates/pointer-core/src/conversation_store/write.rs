//! Incremental write paths (P0 append, P1 meta, P2 sync/replace).

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{ChatMessage, ConversationMeta};

use super::persist::{conversation_preview, message_index_content, role_str};

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
           computer_monitor_id, workspace_root, workspace_user_set, workspace_inherit_disabled,
           lead_agent_id, agent_mode
         ) VALUES (?1,?2,?3,?4,
           COALESCE((SELECT message_count FROM conversations WHERE id = ?1), 0),
           ?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
         ON CONFLICT(id) DO UPDATE SET
           title = excluded.title,
           updated_at_ms = excluded.updated_at_ms,
           skill_ids_json = excluded.skill_ids_json,
           tool_rounds_used = excluded.tool_rounds_used,
           tool_rounds_used_supervisor = excluded.tool_rounds_used_supervisor,
           computer_monitor_id = excluded.computer_monitor_id,
           workspace_root = excluded.workspace_root,
           workspace_user_set = excluded.workspace_user_set,
           workspace_inherit_disabled = excluded.workspace_inherit_disabled,
           lead_agent_id = excluded.lead_agent_id,
           agent_mode = excluded.agent_mode",
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
            i64::from(meta.workspace_user_set),
            i64::from(meta.workspace_inherit_disabled),
            meta.lead_agent_id,
            meta.agent_mode,
        ],
    )?;
    Ok(())
}

/// Pure upsert of conversation shell fields (no messages, no deletion).
///
/// With cursor-paginated lazy loading, the frontend only holds a subset of
/// conversations at any time, so we must NOT delete rows whose ids are absent
/// from `metas` — they may simply be on a not-yet-loaded page. Deletion is
/// handled explicitly via `delete_conversation`.
pub fn save_meta_all_in_conn(conn: &Connection, metas: &[ConversationMeta]) -> Result<()> {
    for meta in metas {
        upsert_conversation_meta(conn, meta)?;
    }
    Ok(())
}

pub(crate) fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn default_conversation_title(conversation_id: &str) -> String {
    crate::channel_outbound::im_conversation_title(conversation_id, None, None)
        .unwrap_or_else(|| "新会话".into())
}

pub(crate) fn ensure_conversation_row(conn: &Connection, conversation_id: &str) -> Result<()> {
    ensure_conversation_row_with_title(conn, conversation_id, None)
}

/// Ensure a conversation meta row exists, creating it with the given title if
/// missing. Used by the cron scheduler to lazily create a cron job's dedicated
/// isolated session (`cron:{job_id}`) on first fire.
pub(crate) fn ensure_conversation_row_with_title(
    conn: &Connection,
    conversation_id: &str,
    title: Option<&str>,
) -> Result<()> {
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
    let title = title
        .map(str::to_string)
        .unwrap_or_else(|| default_conversation_title(conversation_id));
    upsert_conversation_meta(
        conn,
        &ConversationMeta {
            id: conversation_id.to_string(),
            title,
            created_at: now,
            updated_at: now,
            skill_ids: vec![],
            tool_rounds_used: 0,
            tool_rounds_used_supervisor: 0,
            computer_monitor_id: None,
            workspace_root: String::new(),
            workspace_user_set: false,
            workspace_inherit_disabled: false,
            lead_agent_id: crate::agents::DEFAULT_LEAD_AGENT_ID.to_string(),
            agent_mode: crate::agents::AGENT_MODE_SINGLE.to_string(),
            message_count: 0,
            preview: String::new(),
        },
    )
}

pub fn patch_session_agent_in_conn(
    conn: &Connection,
    conversation_id: &str,
    lead_agent_id: &str,
    agent_mode: &str,
) -> Result<()> {
    ensure_conversation_row(conn, conversation_id)?;
    conn.execute(
        "UPDATE conversations SET lead_agent_id = ?2, agent_mode = ?3, updated_at_ms = ?4 WHERE id = ?1",
        params![conversation_id, lead_agent_id, agent_mode, now_ms()],
    )?;
    Ok(())
}

pub fn patch_title_if_default_in_conn(
    conn: &Connection,
    conversation_id: &str,
    title: &str,
) -> Result<()> {
    if title.trim().is_empty() {
        return Ok(());
    }
    let updated = conn.execute(
        "UPDATE conversations SET title = ?2 WHERE id = ?1 AND title = '新会话'",
        params![conversation_id, title],
    )?;
    if updated == 0 {
        ensure_conversation_row_with_title(conn, conversation_id, Some(title))?;
    }
    Ok(())
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

pub fn message_count_in_conn(conn: &Connection, conversation_id: &str) -> Result<u32> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1",
        params![conversation_id],
        |row| row.get(0),
    )?;
    Ok(count as u32)
}

pub fn stored_preview_in_conn(conn: &Connection, conversation_id: &str) -> Result<String> {
    let preview: Option<String> = conn
        .query_row(
            "SELECT preview FROM conversations WHERE id = ?1",
            params![conversation_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    Ok(preview.unwrap_or_default())
}

pub fn flush_conversation_meta_in_conn(
    conn: &Connection,
    conversation_id: &str,
    message_count: u32,
    preview: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE conversations SET message_count = ?2, preview = ?3 WHERE id = ?1",
        params![conversation_id, message_count as i64, preview],
    )?;
    let trimmed = preview.trim();
    if !trimmed.is_empty() {
        let title = super::persist::truncate_chars(trimmed, 24);
        patch_title_if_default_in_conn(conn, conversation_id, &title)?;
    }
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
        log::debug!(
            "conversation_store: append_missing conversation_id={conversation_id} new_messages={written}"
        );
    }
    Ok(written)
}

/// P0: upsert without reloading the full transcript for stats.
pub fn upsert_message_no_refresh_in_conn(
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
    Ok(())
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
    let count = message_count_in_conn(conn, conversation_id)?;
    let preview = stored_preview_in_conn(conn, conversation_id)?;
    flush_conversation_meta_in_conn(conn, conversation_id, count, &preview)?;
    Ok(())
}

pub fn sync_messages_ordered_with_meta_in_conn(
    conn: &Connection,
    conversation_id: &str,
    messages: &[ChatMessage],
    message_count: u32,
    preview: &str,
) -> Result<()> {
    for (pos, msg) in messages.iter().enumerate() {
        insert_message_at(conn, conversation_id, msg, pos as i64)?;
    }
    flush_conversation_meta_in_conn(conn, conversation_id, message_count, preview)?;
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
    let count = messages.len() as u32;
    let preview = conversation_preview(messages);
    flush_conversation_meta_in_conn(conn, conversation_id, count, &preview)?;
    log::info!(
        "conversation_store: replace_messages conversation_id={conversation_id} count={}",
        messages.len()
    );
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
            workspace_user_set: true,
            workspace_inherit_disabled: false,
            lead_agent_id: crate::agents::DEFAULT_LEAD_AGENT_ID.to_string(),
            agent_mode: crate::agents::AGENT_MODE_SINGLE.to_string(),
            message_count: 0,
            preview: String::new(),
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
        let preview = conversation_preview(&conv.messages);
        store
            .sync_messages_ordered_with_meta(
                "c1",
                &conv.messages,
                conv.messages.len() as u32,
                &preview,
            )
            .unwrap();
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded[0].messages.len(), 3);
        assert_eq!(
            loaded[0].messages[0].context_state.as_ref().map(|s| s.included),
            Some(false)
        );
    }

    #[test]
    fn flush_meta_patches_default_title_from_preview() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("c1", "新会话", "hello");
        conv.title = "新会话".into();
        store.save_all(&[conv.clone()]).unwrap();
        store
            .flush_conversation_meta("c1", 2, "hello from user")
            .unwrap();
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded[0].title, "hello from user");
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
