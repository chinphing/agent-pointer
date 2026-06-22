//! Conversation/message persistence helpers.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{ChatMessage, Conversation, Role};

pub fn load_all_from_conn(conn: &Connection) -> Result<Vec<Conversation>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, created_at_ms, updated_at_ms, skill_ids_json,
                tool_rounds_used, tool_rounds_used_supervisor, computer_monitor_id, workspace_root,
                workspace_user_set, workspace_inherit_disabled, lead_agent_id, agent_mode
         FROM conversations
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
