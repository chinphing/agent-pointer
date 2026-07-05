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

pub fn session_user_ids_match(stored: &str, filter: &str) -> bool {
    normalize_session_user_id(stored) == normalize_session_user_id(filter)
}

pub fn conversation_owned_by_session_user_in_conn(
    conn: &Connection,
    conversation_id: &str,
    filter_user_id: &str,
) -> Result<bool> {
    let stored = session_user_id_in_conn(conn, conversation_id)?;
    Ok(session_user_ids_match(&stored, filter_user_id))
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
               workspace_root TEXT NOT NULL DEFAULT '',
               workspace_user_set INTEGER NOT NULL DEFAULT 0,
               workspace_inherit_disabled INTEGER NOT NULL DEFAULT 0,
               lead_agent_id TEXT NOT NULL DEFAULT 'general',
               agent_mode TEXT NOT NULL DEFAULT 'single',
               session_user_id TEXT NOT NULL DEFAULT ''
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
