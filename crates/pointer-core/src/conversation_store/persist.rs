//! Conversation/message persistence helpers.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{
    ChatMessage, Conversation, ConversationMeta, Project, ProjectCursor, ProjectPage, Role,
};
use super::ListScope;

/// Scalar anchor probe row for turn paging: only real user-turn anchors.
/// The SQL already filters `role='user' AND is_system_generated=0`, so every
/// row returned here is a turn anchor — no content/payload deserialization.
/// Only `position` is needed for window math (ids resolved separately when
/// looking up around-targets).
pub(crate) struct AnchorProbeRow {
    pub position: i64,
}

/// Whether a message row is a system-generated/special user message that must
/// NOT count as a turn anchor: injected or synthetic user content (screenshots,
/// compression summaries, trim placeholders) or a scoped sub-message.
pub fn is_system_generated_user_message(msg: &ChatMessage) -> bool {
    matches!(msg.role, Role::User)
        && (crate::task_board::history_trim::is_injected_or_synthetic_user_content(&msg.content)
            || crate::models::is_scoped_sub_message(msg))
}

const SYSTEM_GENERATED_BACKFILL_META: &str = "is_system_generated_backfilled";
const CONTEXT_INCLUDED_BACKFILL_META: &str = "context_included_backfilled";

/// Column value mirroring [`crate::message_context::is_context_included`].
pub fn context_included_column_value(msg: &ChatMessage) -> i64 {
    i64::from(crate::message_context::is_context_included(msg))
}

/// One-time migration: materialize `context_included` for existing rows.
///
/// Set-based `json_extract` update (only flips 1→0). Avoids deserializing every
/// payload into `ChatMessage` on launch for multi‑GB local DBs.
/// `json_valid(payload) = 1` guards against legacy rows with truncated/corrupt
/// JSON payloads — a single malformed row would otherwise fail the whole
/// UPDATE (SQLite raises "malformed JSON") and brick the v22 migration.
pub(crate) fn backfill_context_included(conn: &Connection) -> Result<()> {
    let done: Option<String> = conn
        .query_row(
            "SELECT value FROM store_meta WHERE key = ?1",
            params![CONTEXT_INCLUDED_BACKFILL_META],
            |row| row.get(0),
        )
        .optional()?;
    if done.as_deref() == Some("1") {
        return Ok(());
    }

    log::info!("conversation_store: backfilling context_included (soft-excluded + scoped)");
    let started = std::time::Instant::now();
    let apply = || -> Result<u64> {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let updated = conn.execute(
            "UPDATE messages SET context_included = 0
             WHERE context_included != 0
               AND json_valid(payload) = 1
               AND (
                 json_extract(payload, '$.contextState.included') = 0
                 OR (
                   json_extract(payload, '$.anchorMessageId') IS NOT NULL
                   AND length(trim(json_extract(payload, '$.anchorMessageId'))) > 0
                 )
               )",
            [],
        )? as u64;
        conn.execute(
            "INSERT INTO store_meta(key, value) VALUES (?1, '1')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![CONTEXT_INCLUDED_BACKFILL_META],
        )?;
        conn.execute_batch("COMMIT")?;
        Ok(updated)
    };
    let updated = match apply() {
        Ok(n) => n,
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(e);
        }
    };

    log::info!(
        "conversation_store: context_included backfill done updated={updated} elapsed_ms={}",
        started.elapsed().as_millis()
    );
    Ok(())
}

/// One-time migration: materialize `is_system_generated` for existing rows.
///
/// Only **user** payloads can be system-generated; assistant/tool stay at the
/// column default `0`. Writing every row once (as an early build did) rewrote
/// multi-GB DBs into the WAL on the UI thread and hung production launch.
pub(crate) fn backfill_is_system_generated(conn: &Connection) -> Result<()> {
    let done: Option<String> = conn
        .query_row(
            "SELECT value FROM store_meta WHERE key = ?1",
            params![SYSTEM_GENERATED_BACKFILL_META],
            |row| row.get(0),
        )
        .optional()?;
    if done.as_deref() == Some("1") {
        return Ok(());
    }

    log::info!("conversation_store: backfilling is_system_generated (user rows only)");
    let started = std::time::Instant::now();
    // Prefer `content` over full `payload` when present — synthetic markers live
    // in the text; scoped-sub detection still needs the payload JSON.
    let mut stmt = conn.prepare(
        "SELECT conversation_id, message_id, content, payload
         FROM messages
         WHERE role = 'user' AND is_system_generated = 0",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;

    let mut set_true: Vec<(String, String)> = Vec::new();
    let mut scanned: u64 = 0;
    let mut corrupt: u64 = 0;
    for row in rows {
        let (conversation_id, message_id, content, payload) = row?;
        scanned += 1;
        let flag = if crate::task_board::history_trim::is_injected_or_synthetic_user_content(&content)
        {
            true
        } else {
            match serde_json::from_str::<ChatMessage>(&payload) {
                Ok(msg) => is_system_generated_user_message(&msg),
                Err(_) => {
                    corrupt += 1;
                    log::warn!(
                        "conversation_store: skip corrupt message in backfill_is_system_generated {conversation_id}/{message_id}"
                    );
                    false
                }
            }
        };
        if flag {
            set_true.push((conversation_id, message_id));
        }
    }

    let apply = || -> Result<u64> {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let mut updated: u64 = 0;
        for (conversation_id, message_id) in &set_true {
            let n = conn.execute(
                "UPDATE messages SET is_system_generated = 1
                 WHERE conversation_id = ?1 AND message_id = ?2 AND is_system_generated = 0",
                params![conversation_id, message_id],
            )?;
            updated += n as u64;
        }
        conn.execute(
            "INSERT INTO store_meta(key, value) VALUES (?1, '1')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![SYSTEM_GENERATED_BACKFILL_META],
        )?;
        conn.execute_batch("COMMIT")?;
        Ok(updated)
    };
    let updated = match apply() {
        Ok(n) => n,
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(e);
        }
    };

    log::info!(
        "conversation_store: is_system_generated backfill done scanned_user={scanned} set_true={} updated={updated} corrupt={corrupt} elapsed_ms={}",
        set_true.len(),
        started.elapsed().as_millis()
    );
    Ok(())
}

pub fn load_all_from_conn(conn: &Connection) -> Result<Vec<Conversation>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, created_at_ms, updated_at_ms, skill_ids_json,
                tool_rounds_used, tool_rounds_used_supervisor, computer_monitor_id, project_id, workspace_root,
                workspace_user_set, workspace_inherit_disabled, lead_agent_id, agent_mode,
                session_user_id, is_pinned
         FROM conversations
         WHERE id NOT LIKE 'cron:%'
           AND id NOT LIKE 'webhook:%'
         ORDER BY is_pinned DESC, updated_at_ms DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, u32>(5)?,
            row.get::<_, u32>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, i64>(10)? != 0,
            row.get::<_, i64>(11)? != 0,
            row.get::<_, String>(12)?,
            row.get::<_, String>(13)?,
            row.get::<_, String>(14)?,
            row.get::<_, i64>(15)? != 0,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (
            id,
            title,
            created_at,
            updated_at,
            skill_ids_json,
            tool_rounds_used,
            tool_rounds_used_supervisor,
            computer_monitor_id,
            project_id,
            workspace_root,
            workspace_user_set,
            workspace_inherit_disabled,
            lead_agent_id,
            agent_mode,
            session_user_id,
            is_pinned,
        ) = row?;
        let skill_ids: Vec<String> = serde_json::from_str(&skill_ids_json).unwrap_or_default();
        let messages = load_messages(conn, &id)?;
        out.push(Conversation {
            id,
            title,
            created_at,
            updated_at,
            is_pinned,
            messages,
            skill_ids,
            tool_rounds_used,
            tool_rounds_used_supervisor,
            computer_monitor_id,
            project_id,
            workspace_root,
            workspace_user_set,
            workspace_inherit_disabled,
            lead_agent_id,
            agent_mode,
            session_user_id,
        });
    }
    Ok(out)
}

pub(crate) fn load_messages(conn: &Connection, conversation_id: &str) -> Result<Vec<ChatMessage>> {
    Ok(load_messages_with_positions(conn, conversation_id)?
        .into_iter()
        .map(|(_, msg)| msg)
        .collect())
}

/// Lead `run_chat` working set via materialized `context_included` (schema v22).
/// Soft-excluded / scoped payloads are not selected, so they are never deserialized
/// into the returned Vec. `db_count` is still the full transcript row count.
pub(crate) fn load_lead_working_messages(
    conn: &Connection,
    conversation_id: &str,
) -> Result<(Vec<ChatMessage>, u32)> {
    let db_count: u32 = conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1",
        params![conversation_id],
        |row| {
            let n: i64 = row.get(0)?;
            Ok(n as u32)
        },
    )?;
    let mut stmt = conn.prepare(
        "SELECT message_id, position, payload FROM messages
         WHERE conversation_id = ?1 AND context_included = 1
         ORDER BY position ASC",
    )?;
    let rows = stmt.query_map(params![conversation_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut working = Vec::new();
    let mut scrub: Vec<(String, i64, ChatMessage)> = Vec::new();
    for row in rows {
        let (message_id, position, payload) = row?;
        match serde_json::from_str::<ChatMessage>(&payload) {
            Ok(mut msg) => {
                if msg.strip_tool_raw_output() {
                    scrub.push((message_id, position, msg.clone()));
                }
                // Belt-and-suspenders if a row's column drifted from payload.
                if !crate::message_context::is_context_included(&msg) {
                    log::warn!(
                        "conversation_store: context_included column stale conversation_id={conversation_id} message_id={}",
                        msg.id
                    );
                    continue;
                }
                working.push(msg);
            }
            Err(e) => {
                log::warn!("conversation_store: skip corrupt message in {conversation_id}: {e}")
            }
        }
    }
    scrub_tool_raw_output(conn, conversation_id, scrub);
    Ok((working, db_count))
}

/// Load transcript rows with SQLite `position` (ascending). Used by turn paging.
pub(crate) fn load_messages_with_positions(
    conn: &Connection,
    conversation_id: &str,
) -> Result<Vec<(i64, ChatMessage)>> {
    let mut stmt = conn.prepare(
        "SELECT message_id, position, payload FROM messages
         WHERE conversation_id = ?1
         ORDER BY position ASC",
    )?;
    let rows = stmt.query_map(params![conversation_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    let mut scrub: Vec<(String, i64, ChatMessage)> = Vec::new();
    for row in rows {
        let (message_id, position, payload) = row?;
        match serde_json::from_str::<ChatMessage>(&payload) {
            Ok(mut msg) => {
                if msg.strip_tool_raw_output() {
                    scrub.push((message_id, position, msg.clone()));
                }
                out.push((position, msg));
            }
            Err(e) => {
                log::warn!("conversation_store: skip corrupt message in {conversation_id}: {e}")
            }
        }
    }
    scrub_tool_raw_output(conn, conversation_id, scrub);
    Ok(out)
}

/// Lightweight anchor probe: real user-turn anchors only. The SQL filters
/// `role='user' AND is_system_generated=0` using the materialized column, so no
/// content/payload deserialization happens and the partial index serves it.
/// Used by turn paging so a page load never materializes the full transcript.
pub(crate) fn load_anchor_probe(
    conn: &Connection,
    conversation_id: &str,
) -> Result<Vec<AnchorProbeRow>> {
    let mut stmt = conn.prepare(
        "SELECT position
         FROM messages
         WHERE conversation_id = ?1 AND role = 'user' AND is_system_generated = 0
         ORDER BY position ASC",
    )?;
    let rows = stmt.query_map(params![conversation_id], |row| row.get::<_, i64>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(AnchorProbeRow { position: row? });
    }
    Ok(out)
}

/// Position of the first message row at or after `position` (any role). Used as
/// the exclusive end bound of `before` windows so assistant/tool rows after the
/// cursor are excluded too (the anchor probe only knows user rows).
pub(crate) fn first_position_at_or_after(
    conn: &Connection,
    conversation_id: &str,
    position: i64,
) -> Result<Option<i64>> {
    conn.query_row(
        "SELECT MIN(position) FROM messages
         WHERE conversation_id = ?1 AND position >= ?2",
        params![conversation_id, position],
        |row| row.get::<_, Option<i64>>(0),
    )
    .map_err(Into::into)
}

/// Position of an arbitrary message row (any role), for `around` windows whose
/// target may be an assistant/tool row not present in the anchor probe.
pub(crate) fn message_position(
    conn: &Connection,
    conversation_id: &str,
    message_id: &str,
) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT position FROM messages
             WHERE conversation_id = ?1 AND message_id = ?2",
            params![conversation_id, message_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?)
}

/// Load only the message rows inside `[start_position, end_position)` (ascending).
/// Still runs the legacy toolRawOutput scrub on the returned window.
pub(crate) fn load_messages_in_position_range(
    conn: &Connection,
    conversation_id: &str,
    start_position: i64,
    end_position: i64,
) -> Result<Vec<(i64, ChatMessage)>> {
    let mut stmt = conn.prepare(
        "SELECT message_id, position, payload FROM messages
         WHERE conversation_id = ?1 AND position >= ?2 AND position < ?3
         ORDER BY position ASC",
    )?;
    let rows = stmt.query_map(
        params![conversation_id, start_position, end_position],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )?;
    let mut out = Vec::new();
    let mut scrub: Vec<(String, i64, ChatMessage)> = Vec::new();
    for row in rows {
        let (message_id, position, payload) = row?;
        match serde_json::from_str::<ChatMessage>(&payload) {
            Ok(mut msg) => {
                if msg.strip_tool_raw_output() {
                    scrub.push((message_id, position, msg.clone()));
                }
                out.push((position, msg));
            }
            Err(e) => {
                log::warn!("conversation_store: skip corrupt message in {conversation_id}: {e}")
            }
        }
    }
    scrub_tool_raw_output(conn, conversation_id, scrub);
    Ok(out)
}

/// Lazy migration shared by all transcript loads: drop legacy multi‑MB
/// `toolRawOutput` blobs so the next load is cheap.
fn scrub_tool_raw_output(
    conn: &Connection,
    conversation_id: &str,
    scrub: Vec<(String, i64, ChatMessage)>,
) {
    for (message_id, position, msg) in scrub {
        let content = message_index_content(&msg);
        let payload = match msg.to_store_payload_json() {
            Ok(p) => p,
            Err(e) => {
                log::warn!(
                    "conversation_store: scrub toolRawOutput serialize failed conversation_id={conversation_id} message_id={message_id}: {e}"
                );
                continue;
            }
        };
        if let Err(e) = conn.execute(
            "UPDATE messages SET role = ?1, content = ?2, payload = ?3, created_at_ms = ?4, position = ?5
             WHERE conversation_id = ?6 AND message_id = ?7",
            params![
                role_str(&msg.role),
                content,
                payload,
                msg.created_at,
                position,
                conversation_id,
                message_id,
            ],
        ) {
            log::warn!(
                "conversation_store: scrub toolRawOutput rewrite failed conversation_id={conversation_id} message_id={message_id}: {e:#}"
            );
        } else {
            log::info!(
                "conversation_store: scrubbed toolRawOutput conversation_id={conversation_id} message_id={message_id}"
            );
        }
    }
}

/// Cursor for paginated conversation-meta reads. Sort order is
/// `(is_pinned DESC, updated_at_ms DESC, id DESC)`, so the cursor is the last
/// row of the previous page; the next page fetches rows strictly "before" it.
pub type MetaCursor = (i64, String);

fn cursor_pinned_from_conn(conn: &Connection, cursor: Option<&MetaCursor>) -> Result<i64> {
    match cursor {
        Some((_, id)) => Ok(conn
            .query_row(
                "SELECT is_pinned FROM conversations WHERE id = ?1",
                params![id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0)),
        None => Ok(0),
    }
}

pub fn load_project_page_from_conn(
    conn: &Connection,
    scope: &ListScope,
    cursor: Option<ProjectCursor>,
    limit: i64,
) -> Result<ProjectPage> {
    let limit = limit.clamp(1, 100);
    let filter_uid = scope.filter_uid();
    let (cursor_at, cursor_id) = cursor
        .as_ref()
        .map(|c| (Some(c.last_activity_at), Some(c.id.as_str())))
        .unwrap_or((None, None));
    let cursor_pinned = match cursor.as_ref() {
        Some(cursor) => {
            let pinned: Option<i64> = if let Some(uid) = filter_uid {
                conn.query_row(
                    "SELECT is_pinned FROM projects WHERE id = ?1 AND session_user_id = ?2",
                    params![cursor.id, uid],
                    |row| row.get(0),
                )
                .optional()?
            } else {
                conn.query_row(
                    "SELECT is_pinned FROM projects WHERE id = ?1",
                    params![cursor.id],
                    |row| row.get(0),
                )
                .optional()?
            };
            pinned.unwrap_or(0)
        }
        None => 0,
    };
    let mut stmt = conn.prepare(
        "WITH project_activity AS (
           SELECT p.*,
                  COALESCE(
                    (SELECT c.updated_at_ms
                     FROM conversations c
                     WHERE c.project_id = p.id
                     ORDER BY c.updated_at_ms DESC
                     LIMIT 1),
                    p.created_at_ms
                  ) AS last_activity_at_ms
           FROM projects p
           WHERE p.is_archived = 0
             AND (?5 IS NULL OR p.session_user_id = ?5)
         )
         SELECT id, name, workspace_root, is_default, is_pinned, is_archived,
                created_at_ms, updated_at_ms, session_user_id, last_activity_at_ms
         FROM project_activity
         WHERE 1 = 1
           AND (?1 IS NULL
             OR is_pinned < ?3
             OR (is_pinned = ?3
               AND (last_activity_at_ms < ?1
                 OR (last_activity_at_ms = ?1 AND id < ?2))))
         ORDER BY is_pinned DESC, last_activity_at_ms DESC, id DESC
         LIMIT ?4",
    )?;
    let rows = stmt.query_map(
        params![cursor_at, cursor_id, cursor_pinned, limit + 1, filter_uid],
        project_from_row,
    )?;
    let mut items = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    let has_more = items.len() > limit as usize;
    if has_more {
        items.pop();
    }
    let next_cursor = if has_more {
        items.last().map(|p| ProjectCursor {
            last_activity_at: p.last_activity_at,
            id: p.id.clone(),
        })
    } else {
        None
    };
    Ok(ProjectPage { items, next_cursor })
}

pub fn load_sidebar_projects_from_conn(
    conn: &Connection,
    scope: &ListScope,
) -> Result<Vec<Project>> {
    let filter_uid = scope.filter_uid();
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, p.workspace_root, p.is_default, p.is_pinned, p.is_archived,
                p.created_at_ms, p.updated_at_ms, p.session_user_id,
                COALESCE(
                  (SELECT c.updated_at_ms
                   FROM conversations c
                   WHERE c.project_id = p.id
                   ORDER BY c.updated_at_ms DESC
                   LIMIT 1),
                  p.created_at_ms
                ) AS last_activity_at_ms
         FROM projects p
         WHERE p.is_archived = 0
           AND (?1 IS NULL OR p.session_user_id = ?1)
         ORDER BY p.is_pinned DESC, last_activity_at_ms DESC, p.id DESC
         LIMIT 5",
    )?;
    let rows = stmt.query_map(params![filter_uid], project_from_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn load_project_from_conn(
    conn: &Connection,
    id: &str,
    scope: &ListScope,
) -> Result<Option<Project>> {
    let filter_uid = scope.filter_uid();
    conn.query_row(
        "SELECT p.id, p.name, p.workspace_root, p.is_default, p.is_pinned, p.is_archived,
                p.created_at_ms, p.updated_at_ms, p.session_user_id,
                COALESCE(
                  (SELECT c.updated_at_ms
                   FROM conversations c
                   WHERE c.project_id = p.id
                   ORDER BY c.updated_at_ms DESC
                   LIMIT 1),
                  p.created_at_ms
                ) AS last_activity_at_ms
         FROM projects p
         WHERE p.id = ?1
           AND (?2 IS NULL OR p.session_user_id = ?2)",
        params![id, filter_uid],
        project_from_row,
    )
    .optional()
    .map_err(Into::into)
}

fn project_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    let updated_at = row.get(7)?;
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        workspace_root: row.get(2)?,
        is_default: row.get::<_, i64>(3)? != 0,
        is_pinned: row.get::<_, i64>(4)? != 0,
        is_archived: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
        updated_at,
        session_user_id: row.get(8).unwrap_or_default(),
        last_activity_at: row.get(9).unwrap_or(updated_at),
    })
}

pub fn load_project_metas_from_conn(
    conn: &Connection,
    project_id: &str,
    scope: &ListScope,
    cursor: Option<MetaCursor>,
    limit: i64,
) -> Result<Vec<ConversationMeta>> {
    let limit = limit.clamp(1, 500);
    let filter_uid = scope.filter_uid();
    let (cur_ts, cur_id): (Option<i64>, Option<&str>) = match &cursor {
        Some((ts, id)) => (Some(*ts), Some(id.as_str())),
        None => (None, None),
    };
    let cursor_pinned = cursor_pinned_from_conn(conn, cursor.as_ref())?;
    let mut stmt = conn.prepare(
        "SELECT id, title, created_at_ms, updated_at_ms, message_count, preview,
                skill_ids_json, tool_rounds_used, tool_rounds_used_supervisor,
                computer_monitor_id, project_id, workspace_root, workspace_user_set,
                workspace_inherit_disabled, lead_agent_id, agent_mode, session_user_id,
                is_pinned
         FROM conversations
         WHERE id NOT LIKE 'cron:%' AND id NOT LIKE 'webhook:%'
           AND project_id = ?3
           AND (?6 IS NULL OR session_user_id = ?6)
           AND (?1 IS NULL
             OR is_pinned < ?5
             OR (is_pinned = ?5
               AND (updated_at_ms < ?1
                 OR (updated_at_ms = ?1 AND id < ?2))))
         ORDER BY is_pinned DESC, updated_at_ms DESC, id DESC LIMIT ?4",
    )?;
    let rows = stmt.query_map(
        params![cur_ts, cur_id, project_id, limit, cursor_pinned, filter_uid],
        |row| {
            Ok(MetaRow {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                message_count: row.get::<_, i64>(4)? as u32,
                preview: row.get(5)?,
                skill_ids_json: row.get(6)?,
                tool_rounds_used: row.get(7)?,
                tool_rounds_used_supervisor: row.get(8)?,
                computer_monitor_id: row.get(9)?,
                project_id: row.get(10)?,
                workspace_root: row.get(11)?,
                workspace_user_set: row.get::<_, i64>(12)? != 0,
                workspace_inherit_disabled: row.get::<_, i64>(13)? != 0,
                lead_agent_id: row.get(14)?,
                agent_mode: row.get(15)?,
                session_user_id: row.get(16)?,
                is_pinned: row.get::<_, i64>(17)? != 0,
            })
        },
    )?;
    let mut out = Vec::new();
    for row in rows {
        out.push(meta_from_row(row?)?);
    }
    Ok(out)
}

/// Load conversation shells (no messages) with cursor pagination.
///
/// When `cursor` is `None`, returns the most recent page. Otherwise returns
/// rows strictly before `(is_pinned, updated_at_ms, id)` in DESC order. `limit`
/// is clamped to `[1, 500]` for safety. Logs an `info` line per call with the
/// row count and a `warn` per row with corrupt `skill_ids_json` (falls back
/// to `[]`).
///
/// `scope`: platform admin (`ListScope::All`) sees every user; otherwise only
/// conversations owned by that `session_user_id`.
pub fn load_metas_from_conn(
    conn: &Connection,
    scope: &ListScope,
    cursor: Option<MetaCursor>,
    limit: i64,
) -> Result<Vec<ConversationMeta>> {
    let limit = limit.clamp(1, 500);
    let filter_uid = scope.filter_uid();
    let cursor_pinned = cursor_pinned_from_conn(conn, cursor.as_ref())?;
    let mut stmt = conn.prepare(
        "SELECT id, title, created_at_ms, updated_at_ms, message_count, preview,
                skill_ids_json, tool_rounds_used, tool_rounds_used_supervisor,
                computer_monitor_id, project_id, workspace_root, workspace_user_set,
                workspace_inherit_disabled, lead_agent_id, agent_mode, session_user_id,
                is_pinned
         FROM conversations
         WHERE id NOT LIKE 'cron:%'
           AND id NOT LIKE 'webhook:%'
           AND (?5 IS NULL OR session_user_id = ?5)
           AND (?1 IS NULL
             OR is_pinned < ?4
             OR (is_pinned = ?4
               AND (updated_at_ms < ?1
                 OR (updated_at_ms = ?1 AND id < ?2))))
         ORDER BY is_pinned DESC, updated_at_ms DESC, id DESC
         LIMIT ?3",
    )?;
    let (cur_ts, cur_id): (Option<i64>, Option<&str>) = match &cursor {
        Some((ts, id)) => (Some(*ts), Some(id.as_str())),
        None => (None, None),
    };
    let rows = stmt.query_map(
        params![cur_ts, cur_id, limit, cursor_pinned, filter_uid],
        |row| {
            Ok(MetaRow {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                message_count: row.get::<_, i64>(4)? as u32,
                preview: row.get(5)?,
                skill_ids_json: row.get(6)?,
                tool_rounds_used: row.get(7)?,
                tool_rounds_used_supervisor: row.get(8)?,
                computer_monitor_id: row.get(9)?,
                project_id: row.get(10)?,
                workspace_root: row.get(11)?,
                workspace_user_set: row.get::<_, i64>(12)? != 0,
                workspace_inherit_disabled: row.get::<_, i64>(13)? != 0,
                lead_agent_id: row.get(14)?,
                agent_mode: row.get(15)?,
                session_user_id: row.get(16)?,
                is_pinned: row.get::<_, i64>(17)? != 0,
            })
        },
    )?;
    let mut out = Vec::new();
    for row in rows {
        out.push(meta_from_row(row?)?);
    }
    log::info!(
        "conversation_store: load_metas scope={scope:?} cursor={:?} limit={} returned {} rows",
        cursor,
        limit,
        out.len()
    );
    Ok(out)
}

/// Load a single conversation meta row by id (O(log n) via the primary key).
/// Use this instead of `load_all_from_conn` when only one conversation is
/// needed (e.g. the chat-send persist hooks), so message deserialization is
/// avoided entirely.
pub fn load_meta_from_conn(conn: &Connection, id: &str) -> Result<Option<ConversationMeta>> {
    let r = conn
        .query_row(
            "SELECT id, title, created_at_ms, updated_at_ms, message_count, preview,
                    skill_ids_json, tool_rounds_used, tool_rounds_used_supervisor,
                    computer_monitor_id, project_id, workspace_root, workspace_user_set,
                    workspace_inherit_disabled, lead_agent_id, agent_mode, session_user_id,
                    is_pinned
             FROM conversations WHERE id = ?1",
            params![id],
            |row| {
                Ok(MetaRow {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                    message_count: row.get::<_, i64>(4)? as u32,
                    preview: row.get(5)?,
                    skill_ids_json: row.get(6)?,
                    tool_rounds_used: row.get(7)?,
                    tool_rounds_used_supervisor: row.get(8)?,
                    computer_monitor_id: row.get(9)?,
                    project_id: row.get(10)?,
                    workspace_root: row.get(11)?,
                    workspace_user_set: row.get::<_, i64>(12)? != 0,
                    workspace_inherit_disabled: row.get::<_, i64>(13)? != 0,
                    lead_agent_id: row.get(14)?,
                    agent_mode: row.get(15)?,
                    session_user_id: row.get(16)?,
                    is_pinned: row.get::<_, i64>(17)? != 0,
                })
            },
        )
        .optional()?;
    let Some(r) = r else { return Ok(None) };
    let skill_ids: Vec<String> = match serde_json::from_str(&r.skill_ids_json) {
        Ok(v) => v,
        Err(e) => {
            log::warn!(
                "conversation_store: skip corrupt skill_ids_json for {}: {e}",
                r.id
            );
            Vec::new()
        }
    };
    Ok(Some(ConversationMeta {
        id: r.id,
        title: r.title,
        created_at: r.created_at,
        updated_at: r.updated_at,
        is_pinned: r.is_pinned,
        skill_ids,
        tool_rounds_used: r.tool_rounds_used,
        tool_rounds_used_supervisor: r.tool_rounds_used_supervisor,
        computer_monitor_id: r.computer_monitor_id,
        project_id: r.project_id,
        workspace_root: r.workspace_root,
        workspace_user_set: r.workspace_user_set,
        workspace_inherit_disabled: r.workspace_inherit_disabled,
        lead_agent_id: r.lead_agent_id,
        agent_mode: r.agent_mode,
        message_count: r.message_count,
        preview: r.preview,
        session_user_id: r.session_user_id,
    }))
}

/// Return all conversation ids (cheap; avoids loading messages). Used by save
/// paths to snapshot pre-write ids for deletion detection.
pub fn list_all_ids_from_conn(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM conversations")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for id in rows {
        out.push(id?);
    }
    log::debug!(
        "conversation_store: list_all_ids returned {} ids",
        out.len()
    );
    Ok(out)
}

/// Return the most-recently-updated `workspace_root` among conversations other
/// than `exclude_id` with a non-empty workspace. Used by workspace inheritance
/// without loading any messages.
pub fn latest_other_workspace_root_from_conn(
    conn: &Connection,
    exclude_id: &str,
) -> Result<Option<String>> {
    let root: Option<String> = conn
        .query_row(
            "SELECT workspace_root FROM conversations
             WHERE id != ?1 AND workspace_root != ''
             ORDER BY updated_at_ms DESC, id DESC
             LIMIT 1",
            params![exclude_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(root.filter(|s| !s.trim().is_empty()))
}

struct MetaRow {
    id: String,
    title: String,
    created_at: i64,
    updated_at: i64,
    message_count: u32,
    preview: String,
    skill_ids_json: String,
    tool_rounds_used: u32,
    tool_rounds_used_supervisor: u32,
    computer_monitor_id: Option<String>,
    project_id: Option<String>,
    workspace_root: String,
    workspace_user_set: bool,
    workspace_inherit_disabled: bool,
    lead_agent_id: String,
    agent_mode: String,
    session_user_id: String,
    is_pinned: bool,
}

fn meta_from_row(r: MetaRow) -> Result<ConversationMeta> {
    let skill_ids = match serde_json::from_str(&r.skill_ids_json) {
        Ok(v) => v,
        Err(e) => {
            log::warn!(
                "conversation_store: skip corrupt skill_ids_json for {}: {e}",
                r.id
            );
            Vec::new()
        }
    };
    Ok(ConversationMeta {
        id: r.id,
        title: r.title,
        created_at: r.created_at,
        updated_at: r.updated_at,
        is_pinned: r.is_pinned,
        skill_ids,
        tool_rounds_used: r.tool_rounds_used,
        tool_rounds_used_supervisor: r.tool_rounds_used_supervisor,
        computer_monitor_id: r.computer_monitor_id,
        project_id: r.project_id,
        workspace_root: r.workspace_root,
        workspace_user_set: r.workspace_user_set,
        workspace_inherit_disabled: r.workspace_inherit_disabled,
        lead_agent_id: r.lead_agent_id,
        agent_mode: r.agent_mode,
        message_count: r.message_count,
        preview: r.preview,
        session_user_id: r.session_user_id,
    })
}

pub fn replace_all_in_conn(conn: &Connection, list: &[Conversation]) -> Result<()> {
    for conv in list {
        upsert_conversation(conn, conv, true)?;
    }
    Ok(())
}

pub fn upsert_conversation(
    conn: &Connection,
    conv: &Conversation,
    replace_messages: bool,
) -> Result<bool> {
    let unchanged = conn
        .query_row(
            "SELECT updated_at_ms, message_count FROM conversations WHERE id = ?1",
            params![conv.id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;
    if unchanged == Some((conv.updated_at, conv.messages.len() as i64)) {
        return Ok(false);
    }

    let preview = conversation_preview(&conv.messages);
    let skill_ids_json = serde_json::to_string(&conv.skill_ids)?;
    conn.execute(
        "INSERT INTO conversations (
           id, title, created_at_ms, updated_at_ms, message_count, preview,
           skill_ids_json, tool_rounds_used, tool_rounds_used_supervisor,
           computer_monitor_id, project_id, workspace_root, workspace_user_set, workspace_inherit_disabled,
           lead_agent_id, agent_mode, session_user_id, is_pinned
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
         ON CONFLICT(id) DO UPDATE SET
           title = excluded.title,
           created_at_ms = excluded.created_at_ms,
           updated_at_ms = excluded.updated_at_ms,
           message_count = excluded.message_count,
           preview = excluded.preview,
           skill_ids_json = excluded.skill_ids_json,
           tool_rounds_used = excluded.tool_rounds_used,
           tool_rounds_used_supervisor = excluded.tool_rounds_used_supervisor,
           computer_monitor_id = excluded.computer_monitor_id,
           project_id = excluded.project_id,
           workspace_root = excluded.workspace_root,
           workspace_user_set = excluded.workspace_user_set,
           workspace_inherit_disabled = excluded.workspace_inherit_disabled,
           lead_agent_id = excluded.lead_agent_id,
           agent_mode = excluded.agent_mode,
           session_user_id = CASE
             WHEN trim(excluded.session_user_id) != '' THEN excluded.session_user_id
             ELSE conversations.session_user_id
           END,
           is_pinned = excluded.is_pinned",
        params![
            conv.id,
            conv.title,
            conv.created_at,
            conv.updated_at,
            conv.messages.len() as i64,
            preview,
            skill_ids_json,
            conv.tool_rounds_used,
            conv.tool_rounds_used_supervisor,
            conv.computer_monitor_id,
            conv.project_id,
            conv.workspace_root,
            i64::from(conv.workspace_user_set),
            i64::from(conv.workspace_inherit_disabled),
            conv.lead_agent_id,
            conv.agent_mode,
            conv.session_user_id,
            i64::from(conv.is_pinned),
        ],
    )?;

    if replace_messages {
        conn.execute(
            "DELETE FROM messages WHERE conversation_id = ?1",
            params![conv.id],
        )?;
        for (pos, msg) in conv.messages.iter().enumerate() {
            let content = message_index_content(msg);
            let payload = msg.to_store_payload_json()?;
            conn.execute(
                "INSERT INTO messages (
                   conversation_id, message_id, role, content, payload, created_at_ms, position,
                   is_system_generated, context_included
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    conv.id,
                    msg.id,
                    role_str(&msg.role),
                    content,
                    payload,
                    msg.created_at,
                    pos as i64,
                    i64::from(is_system_generated_user_message(msg)),
                    context_included_column_value(msg),
                ],
            )?;
        }
    }
    Ok(true)
}

pub fn message_index_content(msg: &ChatMessage) -> String {
    let base = match msg.role {
        Role::System => String::new(),
        Role::User | Role::Assistant => msg.content.trim().to_string(),
        Role::Tool => {
            let c = msg.content.trim();
            if c.is_empty() {
                String::new()
            } else {
                format!("[tool] {c}")
            }
        }
    };
    append_attachment_index_suffix(base, msg)
}

fn append_attachment_index_suffix(mut base: String, msg: &ChatMessage) -> String {
    let Some(atts) = msg.attachments.as_ref() else {
        return base;
    };
    for att in atts {
        let name = att.file_name.trim();
        let kind = att.kind.trim();
        if kind.is_empty() && name.is_empty() {
            continue;
        }
        let token = if name.is_empty() {
            format!("[attachment {kind}]")
        } else {
            format!("[attachment {kind} {name}]")
        };
        if base.is_empty() {
            base = token;
        } else {
            base.push(' ');
            base.push_str(&token);
        }
    }
    base
}

pub fn conversation_preview(messages: &[ChatMessage]) -> String {
    for msg in messages {
        if crate::models::is_scoped_sub_message(msg) {
            continue;
        }
        if matches!(msg.role, Role::User) {
            let t = msg.content.trim();
            if !t.is_empty() {
                return truncate_chars(t, 160);
            }
        }
    }
    for msg in messages {
        if crate::models::is_scoped_sub_message(msg) {
            continue;
        }
        if matches!(msg.role, Role::Assistant) {
            let t = msg.content.trim();
            if !t.is_empty() {
                return truncate_chars(t, 160);
            }
        }
    }
    String::new()
}

pub(crate) fn truncate_chars(s: &str, max: usize) -> String {
    crate::text_util::truncate_chars_fit(s, max)
}

pub(crate) fn role_str(role: &Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

/// Test-only helper used by [`ConversationStore::save_all`] (test convenience
/// write path). Production deletion is explicit per-conversation.
#[cfg(test)]
pub fn delete_conversations_not_in(conn: &Connection, ids: &[String]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let placeholders = (0..ids.len())
        .map(|i| format!("?{}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("DELETE FROM conversations WHERE id NOT IN ({placeholders})");
    let params: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    conn.execute(&sql, params.as_slice())?;
    Ok(())
}

/// Delete a single conversation row by id. Associated messages are removed
/// via the `messages.conversation_id` foreign-key `ON DELETE CASCADE`.
pub fn delete_conversation_from_conn(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM conversations WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod message_index_tests {
    use super::*;
    use crate::models::{MediaAttachment, Role};

    #[test]
    fn index_includes_attachment_tokens_when_content_empty() {
        let mut msg = msg("m1", Role::User, "", 0);
        msg.attachments = Some(vec![MediaAttachment {
            id: "a1".into(),
            kind: "image".into(),
            mime_type: "image/jpeg".into(),
            file_name: "id-card.jpg".into(),
            size_bytes: 1,
            storage_rel_path: None,
            content_base64: None,
            derived_text: None,
            local_abs_path: None,
            remote_url: None,
            oss_object_key: None,
        }]);
        assert_eq!(
            message_index_content(&msg),
            "[attachment image id-card.jpg]"
        );
    }
}

#[cfg(test)]
pub fn sample_conv(id: &str, title: &str, user_text: &str) -> Conversation {
    Conversation {
        id: id.to_string(),
        title: title.to_string(),
        created_at: 1_700_000_000_000,
        updated_at: 1_700_000_100_000,
        is_pinned: false,
        messages: vec![
            msg("msg_u1", Role::User, user_text, 1_700_000_000_000),
            msg(
                "msg_a1",
                Role::Assistant,
                "Acknowledged.",
                1_700_000_001_000,
            ),
        ],
        skill_ids: vec![],
        tool_rounds_used: 0,
        tool_rounds_used_supervisor: 0,
        computer_monitor_id: None,
        project_id: None,
        workspace_root: String::new(),
        workspace_user_set: false,
        workspace_inherit_disabled: false,
        lead_agent_id: crate::agents::DEFAULT_LEAD_AGENT_ID.to_string(),
        agent_mode: crate::agents::AGENT_MODE_SINGLE.to_string(),
        session_user_id: String::new(),
    }
}

#[cfg(test)]
pub fn msg(id: &str, role: Role, content: &str, created_at: i64) -> ChatMessage {
    ChatMessage {
        id: id.into(),
        role,
        content: content.into(),
        status: "done".into(),
        created_at,
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
        images_base64: None,
        image_slot_labels: None,
        computer_round_screen_rel_path: None,
        ui_bindings: None,
        context_state: None,
        attachments: None,
        anchor_message_id: None,
        trace_id: None,
        task_id: None,
        spawn_depth: None,
    }
}

pub fn get_last_lead_prompt_tokens_from_conn(
    conn: &Connection,
    conversation_id: &str,
) -> Result<Option<u32>> {
    let value: Option<i64> = conn
        .query_row(
            "SELECT last_lead_prompt_tokens FROM conversations WHERE id = ?1",
            params![conversation_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    Ok(value.filter(|&v| v > 0).map(|v| v as u32))
}

pub fn set_last_lead_prompt_tokens_in_conn(
    conn: &Connection,
    conversation_id: &str,
    prompt_tokens: Option<u32>,
) -> Result<()> {
    let value: Option<i64> = prompt_tokens.map(|v| v as i64);
    let n = conn.execute(
        "UPDATE conversations SET last_lead_prompt_tokens = ?2 WHERE id = ?1",
        params![conversation_id, value],
    )?;
    if n == 0 {
        log::warn!(
            "conversation_store: set_last_lead_prompt_tokens skipped missing conversation_id={conversation_id}"
        );
    }
    Ok(())
}
