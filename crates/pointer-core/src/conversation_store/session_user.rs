//! Per-conversation `session_user_id` persistence.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use super::write;

pub fn session_user_id_in_conn(conn: &Connection, conversation_id: &str) -> Result<String> {
    let value: Option<String> = conn
        .query_row(
            "SELECT session_user_id FROM conversations WHERE id = ?1",
            params![conversation_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(value.unwrap_or_default())
}

pub fn set_session_user_id_in_conn(
    conn: &Connection,
    conversation_id: &str,
    user_id: &str,
) -> Result<()> {
    write::ensure_conversation_row(conn, conversation_id)?;
    let normalized = normalize_session_user_id(user_id);
    conn.execute(
        "UPDATE conversations SET session_user_id = ?2 WHERE id = ?1",
        params![conversation_id, normalized],
    )?;
    Ok(())
}

/// Set `session_user_id` only when currently empty.
pub fn normalize_session_user_id(user_id: &str) -> &str {
    user_id.trim()
}

#[cfg(test)]
fn session_user_ids_match(stored: &str, filter: &str) -> bool {
    normalize_session_user_id(stored) == normalize_session_user_id(filter)
}

pub fn ensure_session_user_id_in_conn(
    conn: &Connection,
    conversation_id: &str,
    candidate: &str,
) -> Result<String> {
    let candidate = candidate.trim();
    write::ensure_conversation_row(conn, conversation_id)?;
    let current = session_user_id_in_conn(conn, conversation_id)?;
    if !current.trim().is_empty() {
        return Ok(current);
    }
    if candidate.is_empty() {
        return Ok(String::new());
    }
    set_session_user_id_in_conn(conn, conversation_id, candidate)?;
    Ok(candidate.to_string())
}

/// Locate an IM desktop conversation for cron/IM delivery mirroring.
///
/// Returns `(base_conv_id, desktop_conv_id)` for the most recently touched row
/// whose `session_user_id` matches `peer_user_id` under `{channel}:{account}:…`.
/// Feishu/home binding uses open_id while inbound sessions use chat_id in
/// `conversation_key`, so id reconstruction from outbound alone is unreliable —
/// peer lookup is the stable join.
pub fn find_im_desktop_for_channel_peer_in_conn(
    conn: &Connection,
    channel: &str,
    account_id: &str,
    peer_user_id: &str,
) -> Result<Option<(String, String)>> {
    let peer = normalize_session_user_id(peer_user_id);
    if peer.is_empty() {
        return Ok(None);
    }
    let channel = channel.trim();
    let account_id = account_id.trim();
    if channel.is_empty() || account_id.is_empty() {
        return Ok(None);
    }
    let prefix = format!("{channel}:{account_id}:");
    let like = format!("{prefix}%");
    let mut stmt = conn.prepare(
        "SELECT id FROM conversations
          WHERE session_user_id = ?1
            AND id LIKE ?2
          ORDER BY CASE
                     WHEN im_last_interaction_at_ms > 0 THEN im_last_interaction_at_ms
                     ELSE updated_at_ms
                   END DESC,
                   updated_at_ms DESC
          LIMIT 30",
    )?;
    let ids = stmt
        .query_map(params![peer, like], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    for id in ids {
        let Some(parts) = crate::channel_outbound::parse_im_conversation_parts(&id) else {
            continue;
        };
        if parts.is_group {
            continue;
        }
        if !parts.channel.eq_ignore_ascii_case(channel) {
            continue;
        }
        if parts.account_id != account_id {
            continue;
        }
        let base = crate::channel_outbound::im_base_conversation_id(&id);
        let state = super::im_session::load_im_session_in_conn(conn, &base)?;
        let desktop = super::im_session::resolve_active_desktop_id(&base, &state);
        return Ok(Some((base, desktop)));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(
            "CREATE TABLE conversations (
               id TEXT PRIMARY KEY,
               title TEXT NOT NULL DEFAULT '新会话',
               created_at_ms INTEGER NOT NULL DEFAULT 0,
               updated_at_ms INTEGER NOT NULL DEFAULT 0,
               message_count INTEGER NOT NULL DEFAULT 0,
               preview TEXT NOT NULL DEFAULT '',
               skill_ids_json TEXT NOT NULL DEFAULT '[]',
               tool_rounds_used INTEGER NOT NULL DEFAULT 0,
               tool_rounds_used_supervisor INTEGER NOT NULL DEFAULT 0,
               computer_monitor_id TEXT,
               project_id TEXT,
               workspace_root TEXT NOT NULL DEFAULT '',
               workspace_user_set INTEGER NOT NULL DEFAULT 0,
               workspace_inherit_disabled INTEGER NOT NULL DEFAULT 0,
               lead_agent_id TEXT NOT NULL DEFAULT 'general',
               agent_mode TEXT NOT NULL DEFAULT 'single',
               session_user_id TEXT NOT NULL DEFAULT '',
               is_pinned INTEGER NOT NULL DEFAULT 0
             );",
        )
        .expect("schema");
        conn
    }

    #[test]
    fn ensure_does_not_overwrite_existing() {
        let conn = mem_conn();
        write::ensure_conversation_row(&conn, "c1").expect("row");
        set_session_user_id_in_conn(&conn, "c1", "im-user").expect("set");
        let kept = ensure_session_user_id_in_conn(&conn, "c1", "login-user").expect("ensure");
        assert_eq!(kept, "im-user");
    }

    #[test]
    fn ensure_writes_when_empty() {
        let conn = mem_conn();
        write::ensure_conversation_row(&conn, "c1").expect("row");
        let written = ensure_session_user_id_in_conn(&conn, "c1", "login-user").expect("ensure");
        assert_eq!(written, "login-user");
    }

    #[test]
    fn session_user_ids_match_trims() {
        assert!(session_user_ids_match(" user-a ", "user-a"));
        assert!(session_user_ids_match("", ""));
        assert!(!session_user_ids_match("user-a", "user-b"));
    }
}
