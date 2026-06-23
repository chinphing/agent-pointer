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
               id              TEXT PRIMARY KEY NOT NULL,
               campaign_id     TEXT NOT NULL,
               seq             INTEGER NOT NULL,
               batch_id        TEXT,
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
               finished_at_ms  INTEGER
             );
             CREATE INDEX IF NOT EXISTS idx_work_items_campaign_status
               ON work_items (campaign_id, status, seq);
             CREATE INDEX IF NOT EXISTS idx_work_items_campaign_batch
               ON work_items (campaign_id, batch_id, seq);
             CREATE UNIQUE INDEX IF NOT EXISTS idx_work_items_campaign_seq
               ON work_items (campaign_id, seq);
             CREATE UNIQUE INDEX IF NOT EXISTS idx_work_items_campaign_target_key
               ON work_items (campaign_id, json_extract(payload_json, '$.target_key'))
               WHERE json_extract(payload_json, '$.target_key') IS NOT NULL;",
        )?;
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
        }))
    }

    pub fn upsert(&self, item: &WorkItem) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO work_items (
               id, campaign_id, seq, batch_id, status, title, payload_json,
               retry_count, max_retries, result_ref, result_json, error_message,
               created_at_ms, updated_at_ms, started_at_ms, finished_at_ms
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
             ON CONFLICT(id) DO UPDATE SET
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
                item.id,
                item.campaign_id,
                item.seq,
                item.batch_id,
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

    pub fn delete_campaign(&self, campaign_id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM work_items WHERE campaign_id = ?1",
            params![campaign_id],
        )?;
        Ok(())
    }

    pub fn load_campaign(&self, campaign_id: &str) -> Result<Vec<WorkItem>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, campaign_id, seq, batch_id, status, title, payload_json,
                    retry_count, max_retries, result_ref, result_json, error_message,
                    created_at_ms, updated_at_ms, started_at_ms, finished_at_ms
             FROM work_items WHERE campaign_id = ?1 ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map(params![campaign_id], |row| {
            let status_s: String = row.get(4)?;
            Ok(WorkItem {
                id: row.get(0)?,
                campaign_id: row.get(1)?,
                seq: row.get(2)?,
                batch_id: row.get(3)?,
                status: WorkItemStatus::from_str_loose(&status_s)
                    .unwrap_or(WorkItemStatus::Pending),
                title: row.get(5)?,
                payload_json: row.get(6)?,
                retry_count: row.get(7)?,
                max_retries: row.get(8)?,
                result_ref: row.get(9)?,
                result_json: row.get(10)?,
                error_message: row.get(11)?,
                created_at_ms: row.get(12)?,
                updated_at_ms: row.get(13)?,
                started_at_ms: row.get(14)?,
                finished_at_ms: row.get(15)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn target_key_exists(&self, campaign_id: &str, target_key: &str) -> Result<bool> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT 1 FROM work_items
             WHERE campaign_id = ?1 AND json_extract(payload_json, '$.target_key') = ?2
             LIMIT 1",
        )?;
        let mut rows = stmt.query(params![campaign_id, target_key])?;
        Ok(rows.next()?.is_some())
    }
}
