//! SQLite persistence for task boards.

use super::super::migrate::normalize_stored_value;
use super::super::model::BoardDocument;
use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Arc;

pub struct TaskBoardSqlite {
    conn: Mutex<Connection>,
}

impl TaskBoardSqlite {
    pub fn open(path: PathBuf) -> Result<Arc<Self>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        let conn = Connection::open(&path)
            .with_context(|| format!("open task board db {}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS task_boards (
               store_key TEXT PRIMARY KEY NOT NULL,
               document TEXT NOT NULL,
               updated_at_ms INTEGER NOT NULL
             );",
        )?;
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
        }))
    }

    pub fn load(&self, store_key: &str) -> Result<Option<BoardDocument>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT document FROM task_boards WHERE store_key = ?1",
        )?;
        let mut rows = stmt.query(params![store_key])?;
        if let Some(row) = rows.next()? {
            let text: String = row.get(0)?;
            let v: serde_json::Value = serde_json::from_str(&text)?;
            return Ok(Some(normalize_stored_value(store_key, v)));
        }
        Ok(None)
    }

    pub fn save(&self, store_key: &str, doc: &BoardDocument) -> Result<()> {
        let text = serde_json::to_string(doc)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO task_boards (store_key, document, updated_at_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(store_key) DO UPDATE SET document = excluded.document, updated_at_ms = excluded.updated_at_ms",
            params![store_key, text, now],
        )?;
        Ok(())
    }

    pub fn list_store_keys_by_prefix(&self, prefix: &str) -> Result<Vec<String>> {
        let like = format!("{prefix}%");
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT store_key
             FROM task_boards
             WHERE store_key LIKE ?1
             ORDER BY updated_at_ms DESC",
        )?;
        let mut rows = stmt.query(params![like])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let key: String = row.get(0)?;
            if !key.trim().is_empty() {
                out.push(key);
            }
        }
        Ok(out)
    }
}
