//! SQLite persistence for task boards.

use super::super::migrate::normalize_stored_value;
use super::super::model::BoardDocument;
use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Arc;

const TASK_BOARD_MAX_ROWS: i64 = 1000;
const TASK_BOARD_FORCE_DELETE_BATCH: i64 = 500;
const TASK_BOARD_COMPLETED_RETENTION_MS: i64 = 10 * 24 * 60 * 60 * 1000;

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
        if let Err(e) = self.cleanup_after_save_with_conn(&conn, now) {
            log::warn!("task_board: sqlite cleanup failed after save store_key={store_key}: {e}");
        }
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

    fn cleanup_after_save_with_conn(&self, conn: &Connection, now_ms: i64) -> Result<()> {
        let cutoff = now_ms.saturating_sub(TASK_BOARD_COMPLETED_RETENTION_MS);
        let aged_deleted = self.delete_completed_older_than_with_conn(conn, cutoff)?;
        if aged_deleted > 0 {
            log::info!(
                "task_board: sqlite cleanup removed aged completed rows count={aged_deleted} cutoff_ms={cutoff}"
            );
        }
        let total = self.count_rows_with_conn(conn)?;
        if total <= TASK_BOARD_MAX_ROWS {
            return Ok(());
        }
        let force_deleted =
            self.delete_oldest_completed_with_conn(conn, TASK_BOARD_FORCE_DELETE_BATCH)?;
        log::warn!(
            "task_board: sqlite over capacity total={} max={} force_deleted_old_completed={}",
            total,
            TASK_BOARD_MAX_ROWS,
            force_deleted
        );
        Ok(())
    }

    fn count_rows_with_conn(&self, conn: &Connection) -> Result<i64> {
        let total = conn.query_row("SELECT COUNT(*) FROM task_boards", [], |r| r.get(0))?;
        Ok(total)
    }

    fn delete_completed_older_than_with_conn(&self, conn: &Connection, cutoff_ms: i64) -> Result<usize> {
        let n = conn.execute(
            "DELETE FROM task_boards
             WHERE updated_at_ms < ?1
               AND json_extract(document, '$.meta.status') = 'completed'",
            params![cutoff_ms],
        )?;
        Ok(n)
    }

    fn delete_oldest_completed_with_conn(&self, conn: &Connection, limit: i64) -> Result<usize> {
        let n = conn.execute(
            "DELETE FROM task_boards
             WHERE store_key IN (
               SELECT store_key
               FROM task_boards
               WHERE json_extract(document, '$.meta.status') = 'completed'
               ORDER BY updated_at_ms ASC
               LIMIT ?1
             )",
            params![limit],
        )?;
        Ok(n)
    }
}
