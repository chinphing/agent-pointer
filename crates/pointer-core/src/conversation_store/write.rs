//! Incremental write paths (P0 append, P1 meta, P2 sync/replace).

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{ChatMessage, ConversationMeta};
use serde::Serialize;

use super::background_host_merge::merge_incoming_over_stored;
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
           skill_ids_json, tool_rounds_used,
           computer_monitor_id, project_id, workspace_root, workspace_user_set, workspace_inherit_disabled,
           lead_agent_id, performance_mode, session_user_id, is_pinned
         ) VALUES (?1,?2,?3,?4,
           COALESCE((SELECT message_count FROM conversations WHERE id = ?1), 0),
           ?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
         ON CONFLICT(id) DO UPDATE SET
           title = excluded.title,
           updated_at_ms = excluded.updated_at_ms,
           skill_ids_json = excluded.skill_ids_json,
           tool_rounds_used = excluded.tool_rounds_used,
           computer_monitor_id = excluded.computer_monitor_id,
           project_id = excluded.project_id,
           workspace_root = excluded.workspace_root,
           workspace_user_set = excluded.workspace_user_set,
           workspace_inherit_disabled = excluded.workspace_inherit_disabled,
           lead_agent_id = excluded.lead_agent_id,
           performance_mode = excluded.performance_mode,
           session_user_id = CASE
             WHEN trim(excluded.session_user_id) != '' THEN excluded.session_user_id
             ELSE conversations.session_user_id
           END,
           is_pinned = excluded.is_pinned",
        params![
            meta.id,
            meta.title,
            meta.created_at,
            meta.updated_at,
            preview,
            skill_ids_json,
            meta.tool_rounds_used,
            meta.computer_monitor_id,
            meta.project_id,
            meta.workspace_root,
            i64::from(meta.workspace_user_set),
            i64::from(meta.workspace_inherit_disabled),
            meta.lead_agent_id,
            meta.performance_mode,
            meta.session_user_id,
            i64::from(meta.is_pinned),
        ],
    )?;
    super::persist::mint_lead_agent_instance_if_empty(conn, &meta.id)?;
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
            is_pinned: false,
            skill_ids: vec![],
            tool_rounds_used: 0,
            computer_monitor_id: None,
            project_id: None,
            workspace_root: String::new(),
            workspace_user_set: false,
            workspace_inherit_disabled: false,
            lead_agent_id: crate::agents::DEFAULT_LEAD_AGENT_ID.to_string(),
            performance_mode: None,
            message_count: 0,
            preview: String::new(),
            session_user_id: String::new(),
        },
    )
}

pub fn patch_session_agent_in_conn(
    conn: &Connection,
    conversation_id: &str,
    lead_agent_id: &str,
) -> Result<()> {
    ensure_conversation_row(conn, conversation_id)?;
    conn.execute(
        "UPDATE conversations SET lead_agent_id = ?2, updated_at_ms = ?3 WHERE id = ?1",
        params![conversation_id, lead_agent_id, now_ms()],
    )?;
    super::persist::mint_lead_agent_instance_if_empty(conn, conversation_id)?;
    Ok(())
}

/// Mint a lead thread id if missing. Reuse it for the life of the conversation.
pub fn ensure_lead_agent_instance_in_conn(
    conn: &Connection,
    conversation_id: &str,
) -> Result<String> {
    ensure_conversation_row(conn, conversation_id)?;
    super::persist::mint_lead_agent_instance_if_empty(conn, conversation_id)
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

/// Max ids per existence probe. SQLite's default `SQLITE_MAX_VARIABLE_NUMBER`
/// is 32766; staying far below it keeps one giant caller batch (a whole working
/// history) from tripping the limit, and keeps every probe on the unique index.
const EXISTING_ID_LOOKUP_CHUNK: usize = 500;

/// Which of `message_ids` already exist for `conversation_id`.
///
/// Probes `UNIQUE(conversation_id, message_id)` with an `IN` list, so every
/// lookup is answered from that covering index — no table row reads. Reading
/// *every* id of the conversation instead costs one row read per message, which
/// on multi-GB transcripts is seconds (and the old `ORDER BY position` pushed
/// the plan onto the non-covering `idx_messages_conv_pos`).
///
/// The result is a set, so it never depends on row order: no `ORDER BY` needed.
fn existing_message_ids_for(
    conn: &Connection,
    conversation_id: &str,
    message_ids: &[&str],
) -> Result<std::collections::HashSet<String>> {
    let mut existing: std::collections::HashSet<String> = std::collections::HashSet::new();
    if message_ids.is_empty() {
        return Ok(existing);
    }
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let unique: Vec<&str> = message_ids
        .iter()
        .copied()
        .filter(|id| seen.insert(id))
        .collect();
    for chunk in unique.chunks(EXISTING_ID_LOOKUP_CHUNK) {
        let placeholders = (0..chunk.len())
            .map(|i| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT message_id FROM messages \
             WHERE conversation_id = ?1 AND message_id IN ({placeholders})"
        );
        let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(chunk.len() + 1);
        params.push(&conversation_id as &dyn rusqlite::ToSql);
        for id in chunk {
            params.push(id as &dyn rusqlite::ToSql);
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params.as_slice(), |row| row.get::<_, String>(0))?;
        for row in rows {
            existing.insert(row?);
        }
    }
    Ok(existing)
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

fn load_stored_message(
    conn: &Connection,
    conversation_id: &str,
    message_id: &str,
) -> Result<Option<ChatMessage>> {
    let payload: Option<String> = conn
        .query_row(
            "SELECT payload FROM messages WHERE conversation_id = ?1 AND message_id = ?2",
            params![conversation_id, message_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(payload) = payload else {
        return Ok(None);
    };
    match serde_json::from_str::<ChatMessage>(&payload) {
        Ok(msg) => Ok(Some(msg)),
        Err(err) => {
            log::warn!(
                "conversation_store: skip merge corrupt payload conversation_id={conversation_id} message_id={message_id}: {err}"
            );
            Ok(None)
        }
    }
}

fn insert_message_at(
    conn: &Connection,
    conversation_id: &str,
    msg: &ChatMessage,
    position: i64,
) -> Result<()> {
    let merged;
    let stamped;
    let stored = load_stored_message(conn, conversation_id, &msg.id)?;
    let is_new = stored.is_none();
    let msg = if let Some(stored) = stored {
        merged = merge_incoming_over_stored(msg, &stored);
        &merged
    } else {
        msg
    };
    let msg = if is_new {
        if let Some(owned) = super::persist::stamp_new_lead_message(conn, conversation_id, msg)? {
            stamped = owned;
            &stamped
        } else {
            msg
        }
    } else {
        msg
    };
    let content = message_index_content(msg);
    let payload = msg.to_store_payload_json()?;
    conn.execute(
        "INSERT INTO messages (
           conversation_id, message_id, role, content, payload, created_at_ms, position,
           is_system_generated, context_included, tool_name, agent_instance_id, is_scoped,
           origin
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
         ON CONFLICT(conversation_id, message_id) DO UPDATE SET
           role = excluded.role,
           content = excluded.content,
           payload = excluded.payload,
           created_at_ms = excluded.created_at_ms,
           position = excluded.position,
           is_system_generated = excluded.is_system_generated,
           context_included = excluded.context_included,
           tool_name = excluded.tool_name,
           agent_instance_id = excluded.agent_instance_id,
           is_scoped = excluded.is_scoped,
           origin = excluded.origin",
        params![
            conversation_id,
            msg.id,
            role_str(&msg.role),
            content,
            payload,
            msg.created_at,
            position,
            i64::from(super::persist::is_system_generated_user_message(msg)),
            super::persist::context_included_column_value(msg),
            super::persist::persist_tool_name(msg),
            super::persist::persist_agent_instance_id(msg),
            super::persist::is_scoped_column_value(msg),
            super::persist::origin_column_value(msg),
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

/// O(1) read of the trigger-maintained `conversations.message_count`
/// (primary-key lookup instead of a `COUNT(*)` over the conversation's index
/// range). Missing conversation reads as 0, same as the `COUNT(*)` it replaces.
///
/// [`message_count_in_conn`] stays as the verification/repair channel: it is
/// the ground truth the triggers are asserted against.
pub fn stored_message_count_in_conn(conn: &Connection, conversation_id: &str) -> Result<u32> {
    let count: Option<i64> = conn
        .query_row(
            "SELECT message_count FROM conversations WHERE id = ?1",
            params![conversation_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(count.unwrap_or(0).max(0) as u32)
}

/// `store_meta` flag: the one-shot `message_count` backfill has completed.
pub const MESSAGE_COUNT_BACKFILL_META: &str = "message_count_backfill_v1";

/// Idempotent single-conversation repair: the `WHERE` guard means an
/// already-consistent row is not rewritten.
const REPAIR_MESSAGE_COUNT_SQL: &str = "UPDATE conversations
        SET message_count = (SELECT COUNT(*) FROM messages WHERE conversation_id = ?1)
      WHERE id = ?1
        AND message_count != (SELECT COUNT(*) FROM messages WHERE conversation_id = ?1)";

/// Recompute one conversation's `message_count` from `COUNT(*)`.
///
/// The verification/repair channel for drift (dropped trigger, out-of-band
/// writer). Safe to call repeatedly. Returns the value now stored.
pub fn repair_message_count_in_conn(conn: &Connection, conversation_id: &str) -> Result<u32> {
    conn.execute(REPAIR_MESSAGE_COUNT_SQL, params![conversation_id])?;
    stored_message_count_in_conn(conn, conversation_id)
}

/// [`repair_message_count_in_conn`] for a batch, returning how many rows changed.
///
/// Used by the chunked one-shot backfill, which needs the changed count (a
/// second run over an already-consistent DB must report 0 and write nothing).
pub fn repair_message_counts_in_conn(conn: &Connection, ids: &[String]) -> Result<u32> {
    let mut changed = 0u32;
    for id in ids {
        changed += conn.execute(REPAIR_MESSAGE_COUNT_SQL, params![id])? as u32;
    }
    Ok(changed)
}

/// Has the one-shot `message_count` backfill already run for this database?
pub fn message_count_backfill_done_in_conn(conn: &Connection) -> Result<bool> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM store_meta WHERE key = ?1",
            params![MESSAGE_COUNT_BACKFILL_META],
            |row| row.get(0),
        )
        .optional()?;
    Ok(value.as_deref() == Some("1"))
}

pub fn mark_message_count_backfill_done_in_conn(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT INTO store_meta(key, value) VALUES (?1, '1')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![MESSAGE_COUNT_BACKFILL_META],
    )?;
    Ok(())
}

pub fn count_duplicate_positions_in_conn(conn: &Connection, conversation_id: &str) -> Result<u32> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM (
           SELECT position FROM messages WHERE conversation_id = ?1
           GROUP BY position HAVING COUNT(*) > 1
         )",
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

/// Refresh the conversation preview (and the default title derived from it).
///
/// Deliberately does **not** touch `message_count`: that column is maintained by
/// the `messages_count_ai/ad` triggers, and writing a recomputed value here
/// would undo the whole point of the triggers.
pub fn flush_conversation_meta_in_conn(
    conn: &Connection,
    conversation_id: &str,
    preview: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE conversations SET preview = ?2 WHERE id = ?1",
        params![conversation_id, preview],
    )?;
    let trimmed = preview.trim();
    if !trimmed.is_empty() {
        let title = super::persist::truncate_chars(trimmed, 24);
        patch_title_if_default_in_conn(conn, conversation_id, &title)?;
    }
    Ok(())
}

/// One row actually written by [`append_missing_messages_in_conn`].
/// Wire-only: lets the frontend attach SQLite positions to in-memory messages
/// after `persistAppend`, without persisting position into payloads.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendedMessageRow {
    pub message_id: String,
    pub position: i64,
}

/// P0: append messages whose ids are not yet in the DB.
pub fn append_missing_messages_in_conn(
    conn: &Connection,
    conversation_id: &str,
    messages: &[ChatMessage],
) -> Result<Vec<AppendedMessageRow>> {
    ensure_conversation_row(conn, conversation_id)?;
    if messages.is_empty() {
        // Zero id lookups and zero position reads when there is nothing to write.
        return Ok(Vec::new());
    }
    let candidate_ids: Vec<&str> = messages.iter().map(|m| m.id.as_str()).collect();
    let existing = existing_message_ids_for(conn, conversation_id, &candidate_ids)?;
    let mut pos = max_message_position(conn, conversation_id)?;
    let mut appended: Vec<AppendedMessageRow> = Vec::new();
    for msg in messages {
        if existing.contains(&msg.id) {
            continue;
        }
        pos += 1;
        insert_message_at(conn, conversation_id, msg, pos)?;
        appended.push(AppendedMessageRow {
            message_id: msg.id.clone(),
            position: pos,
        });
    }
    if !appended.is_empty() {
        log::debug!(
            "conversation_store: append_missing conversation_id={conversation_id} new_messages={}",
            appended.len()
        );
    }
    Ok(appended)
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

pub fn sync_messages_ordered_with_meta_in_conn(
    conn: &Connection,
    conversation_id: &str,
    messages: &[ChatMessage],
    preview: &str,
) -> Result<()> {
    ensure_conversation_row(conn, conversation_id)?;
    let mut existing = message_positions(conn, conversation_id)?;
    let hist_ids: std::collections::HashSet<&str> =
        messages.iter().map(|m| m.id.as_str()).collect();
    let has_db_only = existing.keys().any(|id| !hist_ids.contains(id.as_str()));

    if has_db_only {
        // Soft-excluded (or other) rows remain in DB but not in memory history.
        // Remapping the short list to 0..n-1 would collide with those rows.
        sync_preserving_existing_positions(conn, conversation_id, messages, &mut existing)?;
    } else {
        for (pos, msg) in messages.iter().enumerate() {
            insert_message_at(conn, conversation_id, msg, pos as i64)?;
        }
    }

    // Trigger-maintained column; no `COUNT(*)` here. Logged for parity with the
    // old diagnostic line.
    let count = stored_message_count_in_conn(conn, conversation_id)?;
    flush_conversation_meta_in_conn(conn, conversation_id, preview)?;
    log::info!(
        "conversation_store: sync_messages_ordered conversation_id={conversation_id} list={} db_count={} preserve={}",
        messages.len(),
        count,
        has_db_only
    );
    Ok(())
}

fn message_positions(
    conn: &Connection,
    conversation_id: &str,
) -> Result<std::collections::HashMap<String, i64>> {
    let mut stmt =
        conn.prepare("SELECT message_id, position FROM messages WHERE conversation_id = ?1")?;
    let rows = stmt.query_map(params![conversation_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut out = std::collections::HashMap::new();
    for row in rows {
        let (id, pos) = row?;
        out.insert(id, pos);
    }
    Ok(out)
}

fn shift_positions_from(conn: &Connection, conversation_id: &str, from_pos: i64) -> Result<()> {
    conn.execute(
        "UPDATE messages SET position = position + 1
         WHERE conversation_id = ?1 AND position >= ?2",
        params![conversation_id, from_pos],
    )?;
    Ok(())
}

fn bump_positions_map(existing: &mut std::collections::HashMap<String, i64>, from_pos: i64) {
    for pos in existing.values_mut() {
        if *pos >= from_pos {
            *pos += 1;
        }
    }
}

/// Update payloads for ids already in DB (keep position). Insert brand-new ids
/// before the next list neighbor that already has a DB position; otherwise append.
fn sync_preserving_existing_positions(
    conn: &Connection,
    conversation_id: &str,
    messages: &[ChatMessage],
    existing: &mut std::collections::HashMap<String, i64>,
) -> Result<()> {
    let mut max_pos = max_message_position(conn, conversation_id)?;

    for (i, msg) in messages.iter().enumerate() {
        if let Some(&pos) = existing.get(&msg.id) {
            insert_message_at(conn, conversation_id, msg, pos)?;
            continue;
        }

        let mut insert_pos = None;
        for later in messages.iter().skip(i + 1) {
            if let Some(&p) = existing.get(&later.id) {
                insert_pos = Some(p);
                break;
            }
        }

        let pos = if let Some(p) = insert_pos {
            shift_positions_from(conn, conversation_id, p)?;
            bump_positions_map(existing, p);
            max_pos += 1;
            p
        } else {
            max_pos += 1;
            max_pos
        };
        insert_message_at(conn, conversation_id, msg, pos)?;
        existing.insert(msg.id.clone(), pos);
    }
    Ok(())
}

/// After the last excluded row (keep-window start). Never append at max+1
/// just because the keep-window id is missing from this snapshot.
fn insert_pos_after_excluded(
    positions: &std::collections::HashMap<String, i64>,
    excluded_messages: &[ChatMessage],
    conn: &Connection,
    conversation_id: &str,
) -> Result<i64> {
    let after_excluded = excluded_messages
        .iter()
        .filter_map(|m| positions.get(&m.id).copied())
        .max()
        .map(|p| p + 1);
    let insert_pos = match after_excluded {
        Some(p) => p,
        None => {
            log::warn!(
                "conversation_store: compression insert after excluded empty conversation_id={conversation_id}; using max_position+1"
            );
            max_message_position(conn, conversation_id)? + 1
        }
    };
    shift_positions_from(conn, conversation_id, insert_pos)?;
    Ok(insert_pos)
}

/// Persist context compression without remapping the whole transcript:
/// 1) upsert soft-excluded prefix payloads (positions unchanged)
/// 2) shift rows at/after the cut point by +1
/// 3) insert the summary at the cut-point position
pub fn persist_context_compression_in_conn(
    conn: &Connection,
    conversation_id: &str,
    excluded_messages: &[ChatMessage],
    summary: &ChatMessage,
    insert_before_message_id: &str,
    preview: &str,
) -> Result<()> {
    ensure_conversation_row(conn, conversation_id)?;

    let positions = message_positions(conn, conversation_id)?;
    for msg in excluded_messages {
        if let Some(&pos) = positions.get(&msg.id) {
            insert_message_at(conn, conversation_id, msg, pos)?;
        } else {
            log::warn!(
                "conversation_store: compression exclude skip missing message_id={} conversation_id={}",
                msg.id,
                conversation_id
            );
        }
    }

    let insert_before = insert_before_message_id.trim();
    let insert_pos = if insert_before.is_empty() {
        insert_pos_after_excluded(&positions, excluded_messages, conn, conversation_id)?
    } else if let Some(&p) = positions.get(insert_before) {
        shift_positions_from(conn, conversation_id, p)?;
        p
    } else {
        log::warn!(
            "conversation_store: compression insert_before missing id={} conversation_id={}; inserting after last excluded row",
            insert_before,
            conversation_id
        );
        insert_pos_after_excluded(&positions, excluded_messages, conn, conversation_id)?
    };

    insert_message_at(conn, conversation_id, summary, insert_pos)?;
    let count = stored_message_count_in_conn(conn, conversation_id)?;
    flush_conversation_meta_in_conn(conn, conversation_id, preview)?;
    log::info!(
        "conversation_store: persist_context_compression conversation_id={conversation_id} excluded={} insert_pos={insert_pos} db_count={count}",
        excluded_messages.len()
    );
    Ok(())
}

/// Mark dropped sub-agent rows excluded in place and insert the summary as a
/// scoped row. Does not replace the lead conversation preview.
pub fn persist_sub_agent_compression_in_conn(
    conn: &Connection,
    conversation_id: &str,
    excluded_messages: &[ChatMessage],
    summary: &ChatMessage,
    insert_before_message_id: &str,
    anchor_message_id: &str,
    agent_instance_id: &str,
) -> Result<()> {
    ensure_conversation_row(conn, conversation_id)?;
    let anchor = anchor_message_id.trim();
    let instance = agent_instance_id.trim();
    if anchor.is_empty() || instance.is_empty() {
        log::warn!(
            "conversation_store: sub-agent compression persist skipped conversation_id={conversation_id} anchor_empty={} instance_empty={}",
            anchor.is_empty(),
            instance.is_empty()
        );
        return Ok(());
    }

    let positions = message_positions(conn, conversation_id)?;
    let mut persisted_excluded: Vec<ChatMessage> = Vec::new();
    for msg in excluded_messages {
        let Some(mut stored) = load_stored_message(conn, conversation_id, &msg.id)? else {
            log::warn!(
                "conversation_store: sub-agent compression exclude skip missing message_id={} conversation_id={conversation_id}",
                msg.id
            );
            continue;
        };
        crate::message_context::mark_excluded(
            &mut stored,
            crate::models::ExcludedReason::ContextCompression,
        );
        if let Some(&pos) = positions.get(&stored.id) {
            insert_message_at(conn, conversation_id, &stored, pos)?;
            persisted_excluded.push(stored);
        } else {
            log::warn!(
                "conversation_store: sub-agent compression exclude skip missing position message_id={} conversation_id={conversation_id}",
                msg.id
            );
        }
    }

    let mut summary = summary.clone();
    summary.anchor_message_id = Some(anchor.to_string());
    summary.agent_instance_id = Some(instance.to_string());
    summary.context_state = None;

    let insert_before = insert_before_message_id.trim();
    let insert_pos = if insert_before.is_empty() {
        insert_pos_after_excluded(&positions, &persisted_excluded, conn, conversation_id)?
    } else if let Some(&p) = positions.get(insert_before) {
        shift_positions_from(conn, conversation_id, p)?;
        p
    } else {
        log::warn!(
            "conversation_store: sub-agent compression insert_before missing id={insert_before} conversation_id={conversation_id}"
        );
        insert_pos_after_excluded(&positions, &persisted_excluded, conn, conversation_id)?
    };
    insert_message_at(conn, conversation_id, &summary, insert_pos)?;
    let count = stored_message_count_in_conn(conn, conversation_id)?;
    let preview: String = conn.query_row(
        "SELECT preview FROM conversations WHERE id = ?1",
        params![conversation_id],
        |row| row.get(0),
    )?;
    flush_conversation_meta_in_conn(conn, conversation_id, &preview)?;
    log::info!(
        "conversation_store: persist_sub_agent_compression conversation_id={conversation_id} excluded={} insert_pos={insert_pos} db_count={count} agent_instance_id={instance}",
        persisted_excluded.len()
    );
    Ok(())
}

/// Record the authoritative AI work span of a finished turn on its anchor row.
///
/// `turn_elapsed_ms` is a column, not payload: ordinary transcript upserts
/// (`insert_message_at`) never list it, so a later sync of a client-held copy
/// cannot clear or overwrite it. This is the only writer.
///
/// First write wins (`turn_elapsed_ms IS NULL`): a re-settlement — duplicate
/// Done, retried emit, another run that resolved the same anchor — must not
/// replace a value already measured for that turn. The frontend's "recorded
/// span below 1s loses to the createdAt span" rule covers the one case where
/// the first value is not trustworthy (a cancelled run settling instantly).
///
/// Returns whether the row was written (false = missing row or already set).
pub(crate) fn set_turn_elapsed_ms_in_conn(
    conn: &Connection,
    conversation_id: &str,
    message_id: &str,
    elapsed_ms: i64,
) -> Result<bool> {
    let elapsed_ms = elapsed_ms.max(0);
    let updated = conn.execute(
        "UPDATE messages SET turn_elapsed_ms = ?3
         WHERE conversation_id = ?1 AND message_id = ?2 AND turn_elapsed_ms IS NULL",
        params![conversation_id, message_id, elapsed_ms],
    )?;
    Ok(updated > 0)
}

/// P2b: replace the full transcript for one conversation.
#[cfg_attr(not(test), allow(dead_code))]
pub fn replace_messages_in_conn(
    conn: &Connection,
    conversation_id: &str,
    messages: &[ChatMessage],
) -> Result<()> {
    let milestones = super::persist::snapshot_message_milestones(conn, conversation_id)?;
    conn.execute(
        "DELETE FROM messages WHERE conversation_id = ?1",
        params![conversation_id],
    )?;
    for (pos, msg) in messages.iter().enumerate() {
        insert_message_at(conn, conversation_id, msg, pos as i64)?;
    }
    super::persist::restore_message_milestones(conn, conversation_id, &milestones)?;
    // Row count is trigger-maintained: the DELETE zeroed it, the INSERTs re-added it.
    let preview = conversation_preview(messages);
    flush_conversation_meta_in_conn(conn, conversation_id, &preview)?;
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
    use crate::models::{ExcludedReason, MessageContextState, Role, ToolCall};
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
            is_pinned: false,
            skill_ids: vec![],
            tool_rounds_used: 2,
            computer_monitor_id: None,
            project_id: None,
            workspace_root: "/tmp".into(),
            workspace_user_set: true,
            workspace_inherit_disabled: false,
            lead_agent_id: crate::agents::DEFAULT_LEAD_AGENT_ID.to_string(),
            performance_mode: None,
            message_count: 0,
            preview: String::new(),
            session_user_id: String::new(),
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
            .sync_messages_ordered_with_meta("c1", &conv.messages, &preview)
            .unwrap();
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded[0].messages.len(), 3);
        assert_eq!(
            loaded[0].messages[0]
                .context_state
                .as_ref()
                .map(|s| s.included),
            Some(false)
        );
    }

    #[test]
    fn persist_compression_shifts_suffix_and_keeps_unique_positions() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("c1", "T", "hello");
        // sample_conv: user + assistant. Add a trailing user as cut-point B.
        conv.messages.push(super::super::persist::msg(
            "user_b",
            Role::User,
            "continue",
            3,
        ));
        store.save_all(&[conv.clone()]).unwrap();

        let mut excluded = conv.messages[0].clone();
        excluded.context_state = Some(MessageContextState {
            included: false,
            excluded_reason: Some(ExcludedReason::ContextCompression),
        });
        let mut excluded_asst = conv.messages[1].clone();
        excluded_asst.context_state = Some(MessageContextState {
            included: false,
            excluded_reason: Some(ExcludedReason::ContextCompression),
        });
        let summary = super::super::persist::msg(
            "ctx_test",
            Role::User,
            "[Conversation summary (auto-compression)] x",
            999,
        );

        store
            .persist_context_compression(
                "c1",
                &[excluded, excluded_asst],
                &summary,
                "user_b",
                "preview",
            )
            .unwrap();

        let loaded = store.load_messages("c1").unwrap();
        assert_eq!(loaded.len(), 4);
        assert_eq!(loaded[0].id, conv.messages[0].id);
        assert_eq!(
            loaded[0].context_state.as_ref().map(|s| s.included),
            Some(false)
        );
        assert_eq!(loaded[1].id, conv.messages[1].id);
        assert_eq!(loaded[2].id, "ctx_test");
        assert_eq!(loaded[3].id, "user_b");
        assert_eq!(store.count_duplicate_positions("c1").unwrap(), 0);
    }

    #[test]
    fn persist_compression_missing_insert_before_uses_excluded_cut() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("c1", "T", "hello");
        conv.messages.push(super::super::persist::msg(
            "user_b",
            Role::User,
            "continue",
            3,
        ));
        store.save_all(&[conv.clone()]).unwrap();

        let mut excluded = conv.messages[0].clone();
        excluded.context_state = Some(MessageContextState {
            included: false,
            excluded_reason: Some(ExcludedReason::ContextCompression),
        });
        let mut excluded_asst = conv.messages[1].clone();
        excluded_asst.context_state = Some(MessageContextState {
            included: false,
            excluded_reason: Some(ExcludedReason::ContextCompression),
        });
        let summary = super::super::persist::msg(
            "ctx_missing_anchor",
            Role::User,
            "[Conversation summary (auto-compression)] x",
            999,
        );

        store
            .persist_context_compression(
                "c1",
                &[excluded, excluded_asst],
                &summary,
                "tool-id-not-in-db",
                "preview",
            )
            .unwrap();

        let loaded = store.load_messages("c1").unwrap();
        assert_eq!(loaded.len(), 4);
        assert_eq!(loaded[2].id, "ctx_missing_anchor");
        assert_eq!(loaded[3].id, "user_b");
        assert_eq!(store.count_duplicate_positions("c1").unwrap(), 0);
    }

    #[test]
    fn short_list_sync_with_db_orphans_does_not_collide_positions() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("c1", "T", "hello");
        conv.messages.push(super::super::persist::msg(
            "user_b",
            Role::User,
            "continue",
            3,
        ));
        store.save_all(&[conv.clone()]).unwrap();

        // Simulate post-drain short list that omits the first two rows.
        let short = vec![
            super::super::persist::msg(
                "ctx_new",
                Role::User,
                "[Conversation summary (auto-compression)] x",
                50,
            ),
            conv.messages[2].clone(),
        ];
        store
            .sync_messages_ordered_with_meta("c1", &short, "p")
            .unwrap();

        assert_eq!(store.count_duplicate_positions("c1").unwrap(), 0);
        assert_eq!(store.message_count("c1").unwrap(), 4);
    }

    #[test]
    fn load_lead_working_messages_skips_soft_excluded_keeps_db_count() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("c1", "T", "hello");
        conv.messages[0].context_state = Some(MessageContextState {
            included: false,
            excluded_reason: Some(ExcludedReason::ContextCompression),
        });
        conv.messages[1].context_state = Some(MessageContextState {
            included: false,
            excluded_reason: Some(ExcludedReason::ContextCompression),
        });
        let summary = super::super::persist::msg(
            "ctx_sum",
            Role::User,
            "[Conversation summary (auto-compression)] kept",
            50,
        );
        let recent = super::super::persist::msg("user_b", Role::User, "continue", 60);
        conv.messages.push(summary.clone());
        conv.messages.push(recent.clone());
        store.save_all(&[conv.clone()]).unwrap();

        let flags: Vec<(String, i64)> = {
            let conn = rusqlite::Connection::open(dir.path().join("conversations.db")).unwrap();
            let mut stmt = conn
                .prepare(
                    "SELECT message_id, context_included FROM messages
                     WHERE conversation_id = 'c1' ORDER BY position ASC",
                )
                .unwrap();
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        };
        assert_eq!(
            flags,
            vec![
                (conv.messages[0].id.clone(), 0),
                (conv.messages[1].id.clone(), 0),
                ("ctx_sum".into(), 1),
                ("user_b".into(), 1),
            ]
        );

        let (working, db_count) = store.load_lead_working_messages("c1").unwrap();
        assert_eq!(db_count, 4);
        assert_eq!(
            working.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["ctx_sum", "user_b"]
        );

        store
            .sync_messages_ordered_with_meta("c1", &working, "p")
            .unwrap();
        assert_eq!(store.message_count("c1").unwrap(), 4);
        assert_eq!(store.count_duplicate_positions("c1").unwrap(), 0);
        let full = store.load_messages("c1").unwrap();
        assert_eq!(full.len(), 4);
        assert_eq!(
            full[0].context_state.as_ref().map(|s| s.included),
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
            .flush_conversation_meta("c1", "hello from user")
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

    #[test]
    fn append_missing_returns_appended_rows_with_positions() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let conv = sample_conv("c1", "T", "hello");
        store.save_all(&[conv.clone()]).unwrap();

        let m1 = super::super::persist::msg("m1", Role::User, "one", 101);
        let m2 = super::super::persist::msg("m2", Role::Assistant, "two", 102);
        let rows = store
            .append_missing_messages("c1", &[m1.clone(), m2.clone()])
            .unwrap();
        assert_eq!(rows.len(), 2, "both fresh rows are reported");
        assert_eq!(rows[0].message_id, "m1");
        assert_eq!(rows[1].message_id, "m2");
        assert!(
            rows[1].position > rows[0].position,
            "positions are monotonically increasing"
        );
        let first_position = rows[0].position;

        // Existing ids are skipped and never reported; fresh rows continue after.
        let m3 = super::super::persist::msg("m3", Role::User, "three", 103);
        let rows2 = store
            .append_missing_messages("c1", &[m1, m3.clone()])
            .unwrap();
        assert_eq!(rows2.len(), 1, "duplicate id is skipped");
        assert_eq!(rows2[0].message_id, "m3");
        assert_eq!(
            rows2[0].position,
            first_position + 2,
            "position continues after last row"
        );
    }

    #[test]
    fn append_missing_skips_existing_ids_and_appends_new_ones_in_order() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let conv = sample_conv("c1", "T", "hello");
        let stored_msg = conv.messages[0].clone();
        store.save_all(&[conv]).unwrap();

        let fresh_a = super::super::persist::msg("m_a", Role::User, "a", 301);
        let fresh_b = super::super::persist::msg("m_b", Role::Assistant, "b", 302);
        // The stored id sits in the middle of the batch: only fresh ids are written.
        let rows = store
            .append_missing_messages("c1", &[fresh_a.clone(), stored_msg, fresh_b.clone()])
            .unwrap();
        assert_eq!(
            rows.iter()
                .map(|r| r.message_id.as_str())
                .collect::<Vec<_>>(),
            vec!["m_a", "m_b"],
            "existing id is skipped; fresh ids keep batch order"
        );
        assert!(rows[1].position > rows[0].position);

        // Idempotent: replaying the same batch writes nothing.
        let again = store
            .append_missing_messages("c1", &[fresh_a, fresh_b])
            .unwrap();
        assert!(again.is_empty(), "already-stored ids are not re-reported");
    }

    #[test]
    fn append_missing_empty_input_writes_nothing() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let conv = sample_conv("c1", "T", "hello");
        store.save_all(&[conv]).unwrap();
        let before = store.message_count("c1").unwrap();

        let rows = store.append_missing_messages("c1", &[]).unwrap();
        assert!(rows.is_empty(), "empty input appends nothing");
        assert_eq!(store.message_count("c1").unwrap(), before);

        // Contract kept: the conversation row is still ensured for a fresh id.
        let rows = store.append_missing_messages("fresh_conv", &[]).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn append_missing_duplicate_ids_in_one_batch_keep_todays_behavior() {
        // Pinned: the existence set is built once before the loop, so a repeated
        // id inside one batch is not treated as "already present". Both entries
        // are reported, and the second write lands on the same physical row
        // (ON CONFLICT DO UPDATE) — one row survives, keeping the later position.
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let conv = sample_conv("c1", "T", "hello");
        store.save_all(&[conv]).unwrap();

        let dup = super::super::persist::msg("m_dup", Role::User, "twice", 401);
        let rows = store
            .append_missing_messages("c1", &[dup.clone(), dup.clone()])
            .unwrap();
        assert_eq!(rows.len(), 2, "both batch entries are reported");
        assert_eq!(rows[0].message_id, "m_dup");
        assert_eq!(rows[1].message_id, "m_dup");
        assert_eq!(rows[1].position, rows[0].position + 1);

        let loaded = store.load_messages("c1").unwrap();
        let stored = loaded.iter().filter(|m| m.id == "m_dup").count();
        assert_eq!(stored, 1, "one physical row survives the conflict");
    }

    #[test]
    fn append_missing_chunks_large_batches_without_changing_semantics() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("c1", "T", "hello");
        conv.messages.clear();
        let mut stored = Vec::new();
        for i in 0..600 {
            stored.push(super::super::persist::msg(
                &format!("s{i:04}"),
                Role::User,
                "stored",
                1000 + i,
            ));
        }
        conv.messages.extend(stored.iter().cloned());
        store.save_all(&[conv]).unwrap();

        // 1:1 interleave so stored ids land in every probe chunk (500 + 500 + 200).
        let mut batch = Vec::new();
        let mut fresh_ids = Vec::new();
        for (i, msg) in stored.iter().enumerate() {
            batch.push(msg.clone());
            let fresh = super::super::persist::msg(
                &format!("f{i:04}"),
                Role::Assistant,
                "new",
                2000 + i as i64,
            );
            fresh_ids.push(fresh.id.clone());
            batch.push(fresh);
        }
        let rows = store.append_missing_messages("c1", &batch).unwrap();
        assert_eq!(rows.len(), 600, "only fresh ids are written");
        assert_eq!(
            rows.iter()
                .map(|r| r.message_id.as_str())
                .collect::<Vec<_>>(),
            fresh_ids.iter().map(String::as_str).collect::<Vec<_>>(),
            "fresh ids are appended in batch order across chunks"
        );
        assert!(rows.windows(2).all(|w| w[1].position > w[0].position));
        assert_eq!(store.message_count("c1").unwrap(), 1200);
    }

    #[test]
    fn short_list_sync_does_not_clobber_background_host_success() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let mut conv = sample_conv("c1", "T", "hello");
        let mut host =
            super::super::persist::msg("msg_host", Role::Assistant, "spawn", 1_700_000_002_000);
        host.status = "done".into();
        host.tool_calls = Some(vec![ToolCall {
            id: "call_bg".into(),
            name: "run_subagent".into(),
            arguments: r#"{"agentId":"explore","background":true}"#.into(),
            status: "success".into(),
            result: Some(r#"{"jobId":"job_1","status":"completed","kind":"subagent"}"#.into()),
            error: None,
            duration_ms: Some(10),
            risk_level: None,
            display_label: None,
            display_summary: None,
        }]);
        conv.messages.push(host.clone());
        store.save_all(&[conv.clone()]).unwrap();

        let mut stale = host.clone();
        stale.status = "streaming".into();
        stale.tool_calls.as_mut().unwrap()[0].status = "running".into();
        stale.tool_calls.as_mut().unwrap()[0].result =
            Some(r#"{"jobId":"job_1","status":"running","kind":"subagent"}"#.into());
        let short = vec![conv.messages[0].clone(), stale];
        store
            .sync_messages_ordered_with_meta("c1", &short, "p")
            .unwrap();

        let loaded = store.load_messages("c1").unwrap();
        let got = loaded.iter().find(|m| m.id == "msg_host").unwrap();
        assert_eq!(got.tool_calls.as_ref().unwrap()[0].status, "success");
        assert!(got.tool_calls.as_ref().unwrap()[0]
            .result
            .as_deref()
            .unwrap()
            .contains("completed"));
        assert_eq!(got.status, "done");
    }
}
