//! Conversation/message persistence helpers.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{ChatMessage, Conversation, ConversationMeta, Role};

pub fn load_all_from_conn(conn: &Connection) -> Result<Vec<Conversation>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, created_at_ms, updated_at_ms, skill_ids_json,
                tool_rounds_used, tool_rounds_used_supervisor, computer_monitor_id, workspace_root,
                workspace_user_set, workspace_inherit_disabled, lead_agent_id, agent_mode
         FROM conversations
         WHERE id NOT LIKE 'cron:%'
           AND id NOT LIKE 'webhook:%'
         ORDER BY updated_at_ms DESC",
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
            row.get::<_, String>(8)?,
            row.get::<_, i64>(9)? != 0,
            row.get::<_, i64>(10)? != 0,
            row.get::<_, String>(11)?,
            row.get::<_, String>(12)?,
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
            workspace_root,
            workspace_user_set,
            workspace_inherit_disabled,
            lead_agent_id,
            agent_mode,
        ) = row?;
        let skill_ids: Vec<String> = serde_json::from_str(&skill_ids_json).unwrap_or_default();
        let messages = load_messages(conn, &id)?;
        out.push(Conversation {
            id,
            title,
            created_at,
            updated_at,
            messages,
            skill_ids,
            tool_rounds_used,
            tool_rounds_used_supervisor,
            computer_monitor_id,
            workspace_root,
            workspace_user_set,
            workspace_inherit_disabled,
            lead_agent_id,
            agent_mode,
        });
    }
    Ok(out)
}

pub(crate) fn load_messages(conn: &Connection, conversation_id: &str) -> Result<Vec<ChatMessage>> {
    let mut stmt = conn.prepare(
        "SELECT payload FROM messages
         WHERE conversation_id = ?1
         ORDER BY position ASC",
    )?;
    let rows = stmt.query_map(params![conversation_id], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for payload in rows {
        let payload = payload?;
        match serde_json::from_str::<ChatMessage>(&payload) {
            Ok(msg) => out.push(msg),
            Err(e) => log::warn!(
                "conversation_store: skip corrupt message in {conversation_id}: {e}"
            ),
        }
    }
    Ok(out)
}

/// Cursor for paginated conversation-meta reads. Sort order is
/// `(updated_at_ms DESC, id DESC)`, so the cursor is the last row of the
/// previous page; the next page fetches rows strictly "before" it.
pub type MetaCursor = (i64, String);

/// Load conversation shells (no messages) with cursor pagination.
///
/// When `cursor` is `None`, returns the most recent page. Otherwise returns
/// rows strictly before `(updated_at_ms, id)` in DESC/DESC order. `limit` is
/// clamped to `[1, 500]` for safety. Logs an `info` line per call with the
/// row count and a `warn` per row with corrupt `skill_ids_json` (falls back
/// to `[]`).
pub fn load_metas_from_conn(
    conn: &Connection,
    cursor: Option<MetaCursor>,
    limit: i64,
) -> Result<Vec<ConversationMeta>> {
    let limit = limit.clamp(1, 500);
    let mut stmt = conn.prepare(
        "SELECT id, title, created_at_ms, updated_at_ms, message_count, preview,
                skill_ids_json, tool_rounds_used, tool_rounds_used_supervisor,
                computer_monitor_id, workspace_root, workspace_user_set,
                workspace_inherit_disabled, lead_agent_id, agent_mode
         FROM conversations
         WHERE id NOT LIKE 'cron:%'
           AND id NOT LIKE 'webhook:%'
           AND (?1 IS NULL OR (updated_at_ms < ?1 OR (updated_at_ms = ?1 AND id < ?2)))
         ORDER BY updated_at_ms DESC, id DESC
         LIMIT ?3",
    )?;
    let (cur_ts, cur_id): (Option<i64>, Option<&str>) = match &cursor {
        Some((ts, id)) => (Some(*ts), Some(id.as_str())),
        None => (None, None),
    };
    let rows = stmt.query_map(params![cur_ts, cur_id, limit], |row| {
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
            workspace_root: row.get(10)?,
            workspace_user_set: row.get::<_, i64>(11)? != 0,
            workspace_inherit_disabled: row.get::<_, i64>(12)? != 0,
            lead_agent_id: row.get(13)?,
            agent_mode: row.get(14)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        let r = row?;
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
        out.push(ConversationMeta {
            id: r.id,
            title: r.title,
            created_at: r.created_at,
            updated_at: r.updated_at,
            skill_ids,
            tool_rounds_used: r.tool_rounds_used,
            tool_rounds_used_supervisor: r.tool_rounds_used_supervisor,
            computer_monitor_id: r.computer_monitor_id,
            workspace_root: r.workspace_root,
            workspace_user_set: r.workspace_user_set,
            workspace_inherit_disabled: r.workspace_inherit_disabled,
            lead_agent_id: r.lead_agent_id,
            agent_mode: r.agent_mode,
            message_count: r.message_count,
            preview: r.preview,
        });
    }
    log::info!(
        "conversation_store: load_metas cursor={:?} limit={} returned {} rows",
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
                    computer_monitor_id, workspace_root, workspace_user_set,
                    workspace_inherit_disabled, lead_agent_id, agent_mode
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
                    workspace_root: row.get(10)?,
                    workspace_user_set: row.get::<_, i64>(11)? != 0,
                    workspace_inherit_disabled: row.get::<_, i64>(12)? != 0,
                    lead_agent_id: row.get(13)?,
                    agent_mode: row.get(14)?,
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
        skill_ids,
        tool_rounds_used: r.tool_rounds_used,
        tool_rounds_used_supervisor: r.tool_rounds_used_supervisor,
        computer_monitor_id: r.computer_monitor_id,
        workspace_root: r.workspace_root,
        workspace_user_set: r.workspace_user_set,
        workspace_inherit_disabled: r.workspace_inherit_disabled,
        lead_agent_id: r.lead_agent_id,
        agent_mode: r.agent_mode,
        message_count: r.message_count,
        preview: r.preview,
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
    log::debug!("conversation_store: list_all_ids returned {} ids", out.len());
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
    workspace_root: String,
    workspace_user_set: bool,
    workspace_inherit_disabled: bool,
    lead_agent_id: String,
    agent_mode: String,
}

pub fn replace_all_in_conn(conn: &Connection, list: &[Conversation]) -> Result<()> {
    for conv in list {
        upsert_conversation(conn, conv, true)?;
    }
    Ok(())
}

pub fn upsert_conversation(conn: &Connection, conv: &Conversation, replace_messages: bool) -> Result<bool> {
    let unchanged = conn
        .query_row(
            "SELECT updated_at_ms, message_count FROM conversations WHERE id = ?1",
            params![conv.id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;
    if unchanged
        == Some((
            conv.updated_at,
            conv.messages.len() as i64,
        ))
    {
        return Ok(false);
    }

    let preview = conversation_preview(&conv.messages);
    let skill_ids_json = serde_json::to_string(&conv.skill_ids)?;
    conn.execute(
        "INSERT INTO conversations (
           id, title, created_at_ms, updated_at_ms, message_count, preview,
           skill_ids_json, tool_rounds_used, tool_rounds_used_supervisor,
           computer_monitor_id, workspace_root, workspace_user_set, workspace_inherit_disabled,
           lead_agent_id, agent_mode
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
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
           workspace_root = excluded.workspace_root,
           workspace_user_set = excluded.workspace_user_set,
           workspace_inherit_disabled = excluded.workspace_inherit_disabled,
           lead_agent_id = excluded.lead_agent_id,
           agent_mode = excluded.agent_mode",
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
            conv.workspace_root,
            i64::from(conv.workspace_user_set),
            i64::from(conv.workspace_inherit_disabled),
            conv.lead_agent_id,
            conv.agent_mode,
        ],
    )?;

    if replace_messages {
        conn.execute(
            "DELETE FROM messages WHERE conversation_id = ?1",
            params![conv.id],
        )?;
        for (pos, msg) in conv.messages.iter().enumerate() {
            let content = message_index_content(msg);
            let payload = serde_json::to_string(msg)?;
            conn.execute(
                "INSERT INTO messages (
                   conversation_id, message_id, role, content, payload, created_at_ms, position
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    conv.id,
                    msg.id,
                    role_str(&msg.role),
                    content,
                    payload,
                    msg.created_at,
                    pos as i64,
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
    if s.chars().count() <= max {
        return s.to_string();
    }
    let end: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{end}…")
}

pub(crate) fn role_str(role: &Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

pub fn delete_conversations_not_in(conn: &Connection, ids: &[String]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let placeholders = (0..ids.len())
        .map(|i| format!("?{}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("DELETE FROM conversations WHERE id NOT IN ({placeholders})");
    let params: Vec<&dyn rusqlite::ToSql> =
        ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
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
        messages: vec![
            msg("msg_u1", Role::User, user_text, 1_700_000_000_000),
            msg("msg_a1", Role::Assistant, "Acknowledged.", 1_700_000_001_000),
        ],
        skill_ids: vec![],
        tool_rounds_used: 0,
        tool_rounds_used_supervisor: 0,
        computer_monitor_id: None,
        workspace_root: String::new(),
        workspace_user_set: false,
        workspace_inherit_disabled: false,
        lead_agent_id: crate::agents::DEFAULT_LEAD_AGENT_ID.to_string(),
        agent_mode: crate::agents::AGENT_MODE_SINGLE.to_string(),
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
