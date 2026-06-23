//! SQLite persistence for work items.

use super::super::model::{WorkItem, WorkItemStatus};
use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Arc;

pub struct WorkItemSqlite {
    conn: Mutex<Connection>,
}

impl WorkItemSqlite {
    pub fn open(path: PathBuf) -> Result<Arc<Self>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let conn = Connection::open(&path)
            .with_context(|| format!("open work items db {}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS work_items (
               store_id        TEXT NOT NULL,
               id              TEXT NOT NULL,
               seq             INTEGER NOT NULL,
               status          TEXT NOT NULL,
               title           TEXT NOT NULL,
               payload_json    TEXT NOT NULL DEFAULT '{}',
               depends_on      TEXT NOT NULL DEFAULT '[]',
               retry_count     INTEGER NOT NULL DEFAULT 0,
               max_retries     INTEGER NOT NULL DEFAULT 2,
               result_ref      TEXT,
               result_json     TEXT,
               error_message   TEXT,
               created_at_ms   INTEGER NOT NULL,
               updated_at_ms   INTEGER NOT NULL,
               started_at_ms   INTEGER,
               finished_at_ms  INTEGER,
               PRIMARY KEY (store_id, id)
             );
             CREATE INDEX IF NOT EXISTS idx_work_items_store_status
               ON work_items (store_id, status, seq);
             CREATE UNIQUE INDEX IF NOT EXISTS idx_work_items_store_seq
               ON work_items (store_id, seq);
             CREATE UNIQUE INDEX IF NOT EXISTS idx_work_items_store_target_key
               ON work_items (store_id, json_extract(payload_json, '$.target_key'))
               WHERE json_extract(payload_json, '$.target_key') IS NOT NULL;",
        )?;
        Self::migrate_legacy_columns(&conn)?;
        Self::migrate_numeric_ids(&conn)?;
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
        }))
    }

    fn migrate_legacy_columns(conn: &Connection) -> Result<()> {
        let mut cols: Vec<String> = conn
            .prepare("PRAGMA table_info(work_items)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();
        if cols.iter().any(|c| c == "campaign_id") && !cols.iter().any(|c| c == "store_id") {
            conn.execute(
                "ALTER TABLE work_items RENAME COLUMN campaign_id TO store_id",
                [],
            )?;
            log::info!("work_items: migrated legacy column campaign_id → store_id");
            cols = conn
                .prepare("PRAGMA table_info(work_items)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .collect();
        }
        if cols.iter().any(|c| c == "batch_id") {
            // batch_id is dropped from v4; legacy column left in place until table rebuild.
            log::info!("work_items: legacy batch_id column present; ignored on read/write");
        }
        Ok(())
    }

    /// Rebuild legacy global `id` PK rows to per-store numeric ids (`id` = seq string).
    fn migrate_numeric_ids(conn: &Connection) -> Result<()> {
        let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version >= 2 {
            return Ok(());
        }
        let ddl: String = conn.query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='work_items'",
            [],
            |r| r.get(0),
        )?;
        if ddl.contains("PRIMARY KEY (store_id, id)") {
            conn.execute("PRAGMA user_version = 2", [])?;
            return Ok(());
        }
        log::info!("work_items: migrating to store-scoped numeric ids");
        conn.execute_batch(
            "CREATE TABLE work_items_numeric (
               store_id        TEXT NOT NULL,
               id              TEXT NOT NULL,
               seq             INTEGER NOT NULL,
               status          TEXT NOT NULL,
               title           TEXT NOT NULL,
               payload_json    TEXT NOT NULL DEFAULT '{}',
               depends_on      TEXT NOT NULL DEFAULT '[]',
               retry_count     INTEGER NOT NULL DEFAULT 0,
               max_retries     INTEGER NOT NULL DEFAULT 2,
               result_ref      TEXT,
               result_json     TEXT,
               error_message   TEXT,
               created_at_ms   INTEGER NOT NULL,
               updated_at_ms   INTEGER NOT NULL,
               started_at_ms   INTEGER,
               finished_at_ms  INTEGER,
               PRIMARY KEY (store_id, id)
             );
             INSERT INTO work_items_numeric (
               store_id, id, seq, status, title, payload_json, depends_on,
               retry_count, max_retries, result_ref, result_json, error_message,
               created_at_ms, updated_at_ms, started_at_ms, finished_at_ms
             )
             SELECT
               store_id, CAST(seq AS TEXT), seq, status, title, payload_json, depends_on,
               retry_count, max_retries, result_ref, result_json, error_message,
               created_at_ms, updated_at_ms, started_at_ms, finished_at_ms
             FROM work_items;
             DROP TABLE work_items;
             ALTER TABLE work_items_numeric RENAME TO work_items;
             CREATE INDEX IF NOT EXISTS idx_work_items_store_status
               ON work_items (store_id, status, seq);
             CREATE UNIQUE INDEX IF NOT EXISTS idx_work_items_store_seq
               ON work_items (store_id, seq);
             CREATE UNIQUE INDEX IF NOT EXISTS idx_work_items_store_target_key
               ON work_items (store_id, json_extract(payload_json, '$.target_key'))
               WHERE json_extract(payload_json, '$.target_key') IS NOT NULL;",
        )?;
        conn.execute("PRAGMA user_version = 2", [])?;
        Ok(())
    }

    pub fn upsert(&self, item: &WorkItem) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO work_items (
               id, store_id, seq, status, title, payload_json,
               retry_count, max_retries, result_ref, result_json, error_message,
               created_at_ms, updated_at_ms, started_at_ms, finished_at_ms
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
             ON CONFLICT(store_id, id) DO UPDATE SET
               status = excluded.status,
               title = excluded.title,
               payload_json = excluded.payload_json,
               retry_count = excluded.retry_count,
               result_ref = excluded.result_ref,
               result_json = excluded.result_json,
               error_message = excluded.error_message,
               updated_at_ms = excluded.updated_at_ms,
               started_at_ms = excluded.started_at_ms,
               finished_at_ms = excluded.finished_at_ms",
            params![
                item.store_id,
                item.id,
                item.seq,
                item.status.as_str(),
                item.title,
                item.payload_json,
                item.retry_count,
                item.max_retries,
                item.result_ref,
                item.result_json,
                item.error_message,
                item.created_at_ms,
                item.updated_at_ms,
                item.started_at_ms,
                item.finished_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn delete_store(&self, store_id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM work_items WHERE store_id = ?1",
            params![store_id],
        )?;
        Ok(())
    }

    pub fn load_store(&self, store_id: &str) -> Result<Vec<WorkItem>> {
        let conn = self.conn.lock();
        let has_batch = Self::table_has_column(&conn, "batch_id")?;
        let store_col = if Self::table_has_column(&conn, "store_id")? {
            "store_id"
        } else {
            "campaign_id"
        };
        let sql = if has_batch {
            format!(
                "SELECT id, {store_col}, seq, batch_id, status, title, payload_json,
                        retry_count, max_retries, result_ref, result_json, error_message,
                        created_at_ms, updated_at_ms, started_at_ms, finished_at_ms
                 FROM work_items WHERE {store_col} = ?1 ORDER BY seq ASC"
            )
        } else {
            format!(
                "SELECT id, {store_col}, seq, status, title, payload_json,
                        retry_count, max_retries, result_ref, result_json, error_message,
                        created_at_ms, updated_at_ms, started_at_ms, finished_at_ms
                 FROM work_items WHERE {store_col} = ?1 ORDER BY seq ASC"
            )
        };
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![store_id], |row| {
            let (status_idx, title_idx) = if has_batch { (4, 5) } else { (3, 4) };
            let status_s: String = row.get(status_idx)?;
            Ok(WorkItem {
                id: row.get(0)?,
                store_id: row.get(1)?,
                seq: row.get(2)?,
                status: WorkItemStatus::from_str_loose(&status_s)
                    .unwrap_or(WorkItemStatus::Pending),
                title: row.get(title_idx)?,
                payload_json: row.get(title_idx + 1)?,
                retry_count: row.get(title_idx + 2)?,
                max_retries: row.get(title_idx + 3)?,
                result_ref: row.get(title_idx + 4)?,
                result_json: row.get(title_idx + 5)?,
                error_message: row.get(title_idx + 6)?,
                created_at_ms: row.get(title_idx + 7)?,
                updated_at_ms: row.get(title_idx + 8)?,
                started_at_ms: row.get(title_idx + 9)?,
                finished_at_ms: row.get(title_idx + 10)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    fn table_has_column(conn: &Connection, name: &str) -> Result<bool> {
        let mut stmt = conn.prepare("PRAGMA table_info(work_items)")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let col: String = row.get(1)?;
            if col == name {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn target_key_exists(&self, store_id: &str, target_key: &str) -> Result<bool> {
        let conn = self.conn.lock();
        let col = if Self::table_has_column(&conn, "store_id")? {
            "store_id"
        } else {
            "campaign_id"
        };
        let sql = format!(
            "SELECT 1 FROM work_items
             WHERE {col} = ?1 AND json_extract(payload_json, '$.target_key') = ?2
             LIMIT 1"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params![store_id, target_key])?;
        Ok(rows.next()?.is_some())
    }
}
