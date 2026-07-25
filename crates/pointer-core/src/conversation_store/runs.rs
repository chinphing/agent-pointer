//! `runs` table persistence for the run dispatcher.
//!
//! One row per dispatcher run. Tracks lifecycle status for
//! [`crate::dispatcher::RunDispatcher`] and provides idempotency-key lookup
//! and run status queries for the HTTP Runs API (`GET /api/runs/:id`).
//!
//! Status transitions: `queued` -> `running` -> (`finished` | `failed` | `cancelled`).
//! Each transition logs at info level (project observability rule).
//!
//! Functions take a `&Connection` following the `persist`/`write` module
//! convention; [`crate::conversation_store::ConversationStore`] wraps them with
//! connection locking / write retry.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::dispatcher::TriggerSource;

/// Lifecycle status of a dispatcher run. Stored as TEXT in the `runs` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Queued,
    Running,
    Finished,
    Failed,
    Cancelled,
}

impl RunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Queued => "queued",
            RunStatus::Running => "running",
            RunStatus::Finished => "finished",
            RunStatus::Failed => "failed",
            RunStatus::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            RunStatus::Finished | RunStatus::Failed | RunStatus::Cancelled
        )
    }
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A row read back from the `runs` table.
#[derive(Debug, Clone)]
pub struct RunRecord {
    pub run_id: String,
    pub conversation_id: String,
    pub trigger_source: String,
    pub trigger_meta_json: String,
    pub idempotency_key: Option<String>,
    pub status: String,
    pub created_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    pub error: Option<String>,
    pub summary_json: Option<String>,
}

/// Insert a new run row with `status = queued`. Returns whether the row was
/// inserted (false means a row with the same run_id already existed).
pub fn insert_queued(
    conn: &Connection,
    run_id: &str,
    conversation_id: &str,
    trigger_source: TriggerSource,
    trigger_meta_json: &str,
    idempotency_key: Option<&str>,
) -> Result<bool> {
    let affected = conn.execute(
        "INSERT OR IGNORE INTO runs
           (run_id, conversation_id, trigger_source, trigger_meta_json,
            idempotency_key, status, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6)",
        params![
            run_id,
            conversation_id,
            trigger_source.as_str(),
            trigger_meta_json,
            idempotency_key,
            now_ms() as i64,
        ],
    )?;
    if affected == 0 {
        log::warn!("runs_store: insert_queued ignored duplicate run_id={run_id}");
        return Ok(false);
    }
    log::info!(
        "runs_store: inserted run_id={run_id} conversation_id={conversation_id} source={trigger_source} status=queued"
    );
    Ok(true)
}

/// Find an existing run by idempotency key. Returns the run_id + current
/// status so the dispatcher can reuse it instead of starting a duplicate run.
pub fn find_by_idempotency_key(conn: &Connection, key: &str) -> Result<Option<(String, String)>> {
    let row = conn
        .query_row(
            "SELECT run_id, status FROM runs WHERE idempotency_key = ?1 LIMIT 1",
            params![key],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?;
    Ok(row)
}

pub fn set_status(
    conn: &Connection,
    run_id: &str,
    status: RunStatus,
    error: Option<&str>,
) -> Result<()> {
    let ts = now_ms() as i64;
    match status {
        RunStatus::Queued => {
            // No-op: queued is set at insert time. Defensive: log if called.
            log::warn!("runs_store: set_status(Queued) is a no-op run_id={run_id}");
        }
        RunStatus::Running => {
            conn.execute(
                "UPDATE runs SET status = 'running', started_at_ms = ?2 WHERE run_id = ?1",
                params![run_id, ts],
            )?;
        }
        RunStatus::Finished => {
            conn.execute(
                "UPDATE runs SET status = 'finished', finished_at_ms = ?2 WHERE run_id = ?1",
                params![run_id, ts],
            )?;
        }
        RunStatus::Failed => {
            conn.execute(
                "UPDATE runs SET status = 'failed', finished_at_ms = ?2, error = ?3 WHERE run_id = ?1",
                params![run_id, ts, error],
            )?;
        }
        RunStatus::Cancelled => {
            conn.execute(
                "UPDATE runs SET status = 'cancelled', finished_at_ms = ?2 WHERE run_id = ?1",
                params![run_id, ts],
            )?;
        }
    }
    log::info!(
        "runs_store: run_id={run_id} status -> {}{}",
        status,
        error.map(|e| format!(" error={e}")).unwrap_or_default()
    );
    Ok(())
}

/// Mark runs interrupted by process restart (no in-memory dispatcher task).
pub fn reconcile_interrupted(conn: &Connection) -> Result<u32> {
    let ts = now_ms() as i64;
    let n = conn.execute(
        "UPDATE runs
         SET status = 'cancelled', finished_at_ms = ?1,
             error = COALESCE(error, 'interrupted by server restart')
         WHERE status IN ('queued', 'running')",
        params![ts],
    )?;
    if n > 0 {
        log::info!("runs_store: reconciled {n} interrupted run(s) after restart");
    }
    Ok(n as u32)
}

/// List non-terminal runs for one conversation (queued or running).
pub fn list_non_terminal_by_conversation(
    conn: &Connection,
    conversation_id: &str,
    limit: usize,
) -> Result<Vec<RunRecord>> {
    let limit = limit.max(1).min(50) as i64;
    let mut stmt = conn.prepare(
        "SELECT run_id, conversation_id, trigger_source, trigger_meta_json,
                idempotency_key, status, created_at_ms, started_at_ms,
                finished_at_ms, error, summary_json
         FROM runs
         WHERE conversation_id = ?1 AND status IN ('queued', 'running')
         ORDER BY created_at_ms ASC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![conversation_id, limit], |r| {
        Ok(RunRecord {
            run_id: r.get(0)?,
            conversation_id: r.get(1)?,
            trigger_source: r.get(2)?,
            trigger_meta_json: r.get(3)?,
            idempotency_key: r.get(4)?,
            status: r.get(5)?,
            created_at_ms: r.get(6)?,
            started_at_ms: r.get(7)?,
            finished_at_ms: r.get(8)?,
            error: r.get(9)?,
            summary_json: r.get(10)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// List runs with the given status, oldest first (for queue observability UI).
pub fn list_by_status(conn: &Connection, status: &str, limit: usize) -> Result<Vec<RunRecord>> {
    let limit = limit.max(1).min(200) as i64;
    let mut stmt = conn.prepare(
        "SELECT run_id, conversation_id, trigger_source, trigger_meta_json,
                idempotency_key, status, created_at_ms, started_at_ms,
                finished_at_ms, error, summary_json
         FROM runs
         WHERE status = ?1
         ORDER BY created_at_ms ASC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![status, limit], |r| {
        Ok(RunRecord {
            run_id: r.get(0)?,
            conversation_id: r.get(1)?,
            trigger_source: r.get(2)?,
            trigger_meta_json: r.get(3)?,
            idempotency_key: r.get(4)?,
            status: r.get(5)?,
            created_at_ms: r.get(6)?,
            started_at_ms: r.get(7)?,
            finished_at_ms: r.get(8)?,
            error: r.get(9)?,
            summary_json: r.get(10)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get(conn: &Connection, run_id: &str) -> Result<Option<RunRecord>> {
    let row = conn
        .query_row(
            "SELECT run_id, conversation_id, trigger_source, trigger_meta_json,
                   idempotency_key, status, created_at_ms, started_at_ms,
                   finished_at_ms, error, summary_json
             FROM runs WHERE run_id = ?1",
            params![run_id],
            |r| {
                Ok(RunRecord {
                    run_id: r.get(0)?,
                    conversation_id: r.get(1)?,
                    trigger_source: r.get(2)?,
                    trigger_meta_json: r.get(3)?,
                    idempotency_key: r.get(4)?,
                    status: r.get(5)?,
                    created_at_ms: r.get(6)?,
                    started_at_ms: r.get(7)?,
                    finished_at_ms: r.get(8)?,
                    error: r.get(9)?,
                    summary_json: r.get(10)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

/// Create the `runs` table (idempotent). Called from `init_schema` on every
/// open; safe to call on existing databases.
pub fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS runs (
           run_id TEXT PRIMARY KEY,
           conversation_id TEXT NOT NULL,
           trigger_source TEXT NOT NULL,
           trigger_meta_json TEXT NOT NULL DEFAULT '{}',
           idempotency_key TEXT,
           status TEXT NOT NULL,
           created_at_ms INTEGER NOT NULL,
           started_at_ms INTEGER,
           finished_at_ms INTEGER,
           error TEXT,
           summary_json TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_runs_by_conv ON runs(conversation_id);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_runs_by_idempotency
           ON runs(idempotency_key) WHERE idempotency_key IS NOT NULL;",
    )?;
    Ok(())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Helper for callers that only need to know whether a run is terminal.
pub fn is_terminal_status_str(s: &str) -> bool {
    matches!(s, "finished" | "failed" | "cancelled")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatcher::TriggerSource;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn insert_and_get_roundtrip() {
        let conn = mem();
        let inserted =
            insert_queued(&conn, "r1", "c1", TriggerSource::Ipc, "{}", Some("idem-1")).unwrap();
        assert!(inserted);
        let dup =
            insert_queued(&conn, "r1", "c1", TriggerSource::Ipc, "{}", Some("idem-1")).unwrap();
        assert!(!dup, "duplicate run_id should be ignored");

        let row = get(&conn, "r1").unwrap().unwrap();
        assert_eq!(row.status, "queued");
        assert_eq!(row.trigger_source, "ipc");

        let found = find_by_idempotency_key(&conn, "idem-1").unwrap();
        assert_eq!(found, Some(("r1".to_string(), "queued".to_string())));
    }

    #[test]
    fn set_status_transitions() {
        let conn = mem();
        insert_queued(&conn, "r2", "c2", TriggerSource::Cron, "{}", None).unwrap();
        set_status(&conn, "r2", RunStatus::Running, None).unwrap();
        assert_eq!(get(&conn, "r2").unwrap().unwrap().status, "running");
        set_status(&conn, "r2", RunStatus::Failed, Some("boom")).unwrap();
        let row = get(&conn, "r2").unwrap().unwrap();
        assert_eq!(row.status, "failed");
        assert_eq!(row.error.as_deref(), Some("boom"));
        assert!(is_terminal_status_str(&row.status));
    }

    #[test]
    fn reconcile_interrupted_cancels_queued_and_running() {
        let conn = mem();
        insert_queued(&conn, "q1", "c1", TriggerSource::Webhook, "{}", None).unwrap();
        insert_queued(&conn, "r1", "c2", TriggerSource::HttpRuns, "{}", None).unwrap();
        set_status(&conn, "r1", RunStatus::Running, None).unwrap();
        assert_eq!(reconcile_interrupted(&conn).unwrap(), 2);
        assert_eq!(get(&conn, "q1").unwrap().unwrap().status, "cancelled");
        assert_eq!(get(&conn, "r1").unwrap().unwrap().status, "cancelled");
    }
}
