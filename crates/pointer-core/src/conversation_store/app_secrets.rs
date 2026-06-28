//! `app_secrets` kv table for encrypted app-level secrets (Phase 6 webhook
//! token). Stores opaque encrypted blobs keyed by a string label. The
//! encryption layer lives in [`crate::webhook_config`] (uses
//! [`crate::local_secret`]); this module only persists raw bytes.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

/// Read a secret blob by label, or `None` if unset.
pub fn get(conn: &Connection, label: &str) -> Result<Option<Vec<u8>>> {
    let row = conn
        .query_row(
            "SELECT value FROM app_secrets WHERE label = ?1",
            params![label],
            |r| r.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    Ok(row)
}

/// Whether a secret exists for the label.
pub fn has(conn: &Connection, label: &str) -> Result<bool> {
    Ok(get(conn, label)?.is_some())
}

/// Insert a secret blob. Returns `false` if a secret already exists for the
/// label (first-write-only semantics; callers must clear before re-issuing).
pub fn try_insert(conn: &Connection, label: &str, value: &[u8]) -> Result<bool> {
    let affected = conn.execute(
        "INSERT OR IGNORE INTO app_secrets (label, value, created_at_ms) VALUES (?1, ?2, ?3)",
        params![label, value, now_ms() as i64],
    )?;
    Ok(affected > 0)
}

/// Remove a secret by label. Returns whether a row was deleted. Used for
/// explicit reset (e.g. by a future admin path); the UI is first-write-only.
pub fn delete(conn: &Connection, label: &str) -> Result<bool> {
    let affected = conn.execute("DELETE FROM app_secrets WHERE label = ?1", params![label])?;
    Ok(affected > 0)
}

/// List secret labels (and created_at_ms) whose label starts with `prefix`.
pub fn list_by_prefix(conn: &Connection, prefix: &str) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT label, created_at_ms FROM app_secrets
         WHERE label LIKE ?1 || '%'
         ORDER BY label ASC",
    )?;
    let rows = stmt.query_map(params![prefix], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Create the `app_secrets` table (idempotent). Called from `init_schema`.
pub fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS app_secrets (
           label TEXT PRIMARY KEY,
           value BLOB NOT NULL,
           created_at_ms INTEGER NOT NULL
         );",
    )?;
    Ok(())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn first_write_only() {
        let conn = mem();
        assert!(try_insert(&conn, "k", b"v1").unwrap());
        // Second insert is ignored — first-write-only.
        assert!(!try_insert(&conn, "k", b"v2").unwrap());
        let v = get(&conn, "k").unwrap().unwrap();
        assert_eq!(v, b"v1");
    }

    #[test]
    fn delete_then_reissue() {
        let conn = mem();
        assert!(try_insert(&conn, "k", b"v1").unwrap());
        assert!(delete(&conn, "k").unwrap());
        assert!(try_insert(&conn, "k", b"v2").unwrap());
        assert_eq!(get(&conn, "k").unwrap().unwrap(), b"v2");
    }
}
