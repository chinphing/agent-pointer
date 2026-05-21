//! SQLite-backed LLM token usage: one accumulating row per conversation, pending queue for API flush.

use anyhow::{Context, Result};
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::OnceLock;
use uuid::Uuid;

use crate::llm_token_stats::LlmUsageSnapshot;
use crate::storage::app_data_dir;

const DB_FILE: &str = "token_usage.db";
const LEGACY_PENDING_FILE: &str = "token_usage_pending.jsonl";

static DB: OnceLock<Mutex<Connection>> = OnceLock::new();

fn db_path() -> Result<PathBuf> {
    Ok(app_data_dir()?.join(DB_FILE))
}

fn connection() -> Result<&'static Mutex<Connection>> {
    if DB.get().is_none() {
        let path = db_path()?;
        let conn = Connection::open(&path)
            .with_context(|| format!("open token usage db {}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS usage_accum (
               conversation_id TEXT PRIMARY KEY NOT NULL,
               prompt_tokens INTEGER NOT NULL DEFAULT 0,
               completion_tokens INTEGER NOT NULL DEFAULT 0,
               thinking_tokens INTEGER NOT NULL DEFAULT 0,
               total_tokens INTEGER NOT NULL DEFAULT 0,
               llm_rounds INTEGER NOT NULL DEFAULT 0,
               model_name TEXT,
               period_start TEXT,
               period_end TEXT,
               updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS usage_pending (
               request_id TEXT PRIMARY KEY NOT NULL,
               conversation_id TEXT NOT NULL,
               prompt_tokens INTEGER NOT NULL,
               completion_tokens INTEGER NOT NULL,
               thinking_tokens INTEGER NOT NULL,
               total_tokens INTEGER NOT NULL,
               llm_rounds INTEGER NOT NULL,
               model_name TEXT,
               period_start TEXT,
               period_end TEXT,
               created_at TEXT NOT NULL
             );",
        )?;
        migrate_legacy_jsonl(&conn)?;
        let _ = DB.set(Mutex::new(conn));
    }
    Ok(DB.get().expect("token usage db initialized"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyPendingRow {
    request_id: String,
    prompt_tokens: u32,
    completion_tokens: u32,
    thinking_tokens: u32,
    total_tokens: u32,
    model_name: Option<String>,
    assistant_rounds: Option<u32>,
    period_start: Option<String>,
    period_end: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PendingTokenUsageReport {
    pub request_id: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub thinking_tokens: u32,
    pub total_tokens: u32,
    pub model_name: Option<String>,
    pub assistant_rounds: Option<u32>,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
}

struct AccumRow {
    prompt_tokens: u32,
    completion_tokens: u32,
    thinking_tokens: u32,
    total_tokens: u32,
    llm_rounds: u32,
    model_name: Option<String>,
    period_start: Option<String>,
    period_end: Option<String>,
}

fn migrate_legacy_jsonl(conn: &Connection) -> Result<()> {
    let path = app_data_dir()?.join(LEGACY_PENDING_FILE);
    if !path.exists() {
        return Ok(());
    }
    let f = File::open(&path)?;
    let mut migrated = 0u32;
    for line in BufReader::new(f).lines() {
        let line = line?;
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let row: LegacyPendingRow = match serde_json::from_str(t) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("token_usage_store: skip bad legacy line: {e}");
                continue;
            }
        };
        let rounds = row.assistant_rounds.unwrap_or(1).max(1);
        let now = Utc::now().to_rfc3339();
        let period_start = row.period_start.unwrap_or_else(|| now.clone());
        let period_end = row.period_end.unwrap_or_else(|| now.clone());
        let n = conn.execute(
            "INSERT OR IGNORE INTO usage_pending (
               request_id, conversation_id, prompt_tokens, completion_tokens, thinking_tokens,
               total_tokens, llm_rounds, model_name, period_start, period_end, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                row.request_id,
                "legacy",
                row.prompt_tokens,
                row.completion_tokens,
                row.thinking_tokens,
                row.total_tokens,
                rounds,
                row.model_name,
                period_start,
                period_end,
                now,
            ],
        )?;
        if n > 0 {
            migrated += 1;
        }
    }
    if migrated > 0 {
        log::info!("token_usage_store: migrated {migrated} row(s) from legacy jsonl");
    }
    if let Err(e) = std::fs::rename(&path, path.with_extension("jsonl.migrated")) {
        log::warn!("token_usage_store: rename legacy jsonl failed: {e}");
    }
    Ok(())
}

/// Start a new `run_chat` accumulation for this conversation (finalizes stale accum first).
pub fn begin_run(conversation_id: &str, model_name: Option<&str>) -> Result<()> {
    let guard = connection()?;
    let conn = guard.lock();
    finalize_accum_if_nonzero(&conn, conversation_id)?;
    let now = Utc::now().to_rfc3339();
    let model = model_name.map(str::to_string);
    conn.execute(
        "INSERT INTO usage_accum (
           conversation_id, prompt_tokens, completion_tokens, thinking_tokens, total_tokens,
           llm_rounds, model_name, period_start, period_end, updated_at
         ) VALUES (?1, 0, 0, 0, 0, 0, ?2, NULL, NULL, ?3)
         ON CONFLICT(conversation_id) DO UPDATE SET
           prompt_tokens=0, completion_tokens=0, thinking_tokens=0, total_tokens=0,
           llm_rounds=0, model_name=excluded.model_name, period_start=NULL, period_end=NULL,
           updated_at=excluded.updated_at",
        params![conversation_id, model, now],
    )?;
    Ok(())
}

/// Persist one LLM round into the conversation row (immediate durable write).
pub fn record_round(
    conversation_id: &str,
    usage: Option<&LlmUsageSnapshot>,
    model_name: Option<&str>,
) -> Result<()> {
    let guard = connection()?;
    let conn = guard.lock();
    let now = Utc::now().to_rfc3339();
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM usage_accum WHERE conversation_id = ?1",
            params![conversation_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if exists == 0 {
        let model = model_name.map(str::to_string);
        conn.execute(
            "INSERT INTO usage_accum (
               conversation_id, prompt_tokens, completion_tokens, thinking_tokens, total_tokens,
               llm_rounds, model_name, period_start, period_end, updated_at
             ) VALUES (?1, 0, 0, 0, 0, 0, ?2, NULL, NULL, ?3)",
            params![conversation_id, model, now],
        )?;
    }
    if let Some(u) = usage {
        let model = model_name.map(str::to_string);
        conn.execute(
            "UPDATE usage_accum SET
               prompt_tokens = prompt_tokens + ?2,
               completion_tokens = completion_tokens + ?3,
               thinking_tokens = thinking_tokens + ?4,
               total_tokens = total_tokens + ?5,
               llm_rounds = llm_rounds + 1,
               model_name = COALESCE(?6, model_name),
               period_start = COALESCE(period_start, ?7),
               period_end = ?7,
               updated_at = ?7
             WHERE conversation_id = ?1",
            params![
                conversation_id,
                u.prompt_tokens,
                u.completion_tokens,
                u.reasoning_tokens,
                u.total_tokens,
                model,
                now,
            ],
        )?;
    } else {
        conn.execute(
            "UPDATE usage_accum SET
               llm_rounds = llm_rounds + 1,
               period_start = COALESCE(period_start, ?2),
               period_end = ?2,
               updated_at = ?2
             WHERE conversation_id = ?1",
            params![conversation_id, now],
        )?;
    }
    Ok(())
}

fn read_accum(conn: &Connection, conversation_id: &str) -> Result<Option<AccumRow>> {
    let mut stmt = conn.prepare(
        "SELECT prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
                model_name, period_start, period_end
         FROM usage_accum WHERE conversation_id = ?1",
    )?;
    let mut rows = stmt.query(params![conversation_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(AccumRow {
            prompt_tokens: row.get(0)?,
            completion_tokens: row.get(1)?,
            thinking_tokens: row.get(2)?,
            total_tokens: row.get(3)?,
            llm_rounds: row.get(4)?,
            model_name: row.get(5)?,
            period_start: row.get(6)?,
            period_end: row.get(7)?,
        }))
    } else {
        Ok(None)
    }
}

fn insert_pending_from_accum(
    conn: &Connection,
    conversation_id: &str,
    accum: &AccumRow,
) -> Result<Option<String>> {
    if accum.llm_rounds == 0 {
        return Ok(None);
    }
    let thinking = accum.thinking_tokens;
    let completion = accum.completion_tokens.saturating_sub(thinking);
    let request_id = format!("run:{conversation_id}:{}", Uuid::new_v4());
    let now = Utc::now().to_rfc3339();
    let period_start = accum.period_start.clone().unwrap_or_else(|| now.clone());
    let period_end = accum.period_end.clone().unwrap_or(now);
    conn.execute(
        "INSERT INTO usage_pending (
           request_id, conversation_id, prompt_tokens, completion_tokens, thinking_tokens,
           total_tokens, llm_rounds, model_name, period_start, period_end, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            request_id,
            conversation_id,
            accum.prompt_tokens,
            completion,
            thinking,
            accum.total_tokens,
            accum.llm_rounds,
            accum.model_name,
            period_start,
            period_end,
            Utc::now().to_rfc3339(),
        ],
    )?;
    log::info!(
        "token_usage_store: enqueued request_id={} conversation_id={} total_tokens={}",
        request_id,
        conversation_id,
        accum.total_tokens
    );
    Ok(Some(request_id))
}

fn finalize_accum_if_nonzero(conn: &Connection, conversation_id: &str) -> Result<()> {
    if let Some(accum) = read_accum(conn, conversation_id)? {
        if accum.llm_rounds > 0 {
            insert_pending_from_accum(conn, conversation_id, &accum)?;
        }
    }
    Ok(())
}

/// Move current conversation accumulation into the pending report queue and reset accum.
pub fn finalize_run(conversation_id: &str) -> Result<()> {
    let guard = connection()?;
    let conn = guard.lock();
    if let Some(accum) = read_accum(&conn, conversation_id)? {
        insert_pending_from_accum(&conn, conversation_id, &accum)?;
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE usage_accum SET
           prompt_tokens=0, completion_tokens=0, thinking_tokens=0, total_tokens=0,
           llm_rounds=0, period_start=NULL, period_end=NULL, updated_at=?2
         WHERE conversation_id = ?1",
        params![conversation_id, now],
    )?;
    Ok(())
}

/// On startup: finalize any conversation rows left mid-run (crash recovery).
pub fn finalize_all_stale_accum() -> Result<usize> {
    let guard = connection()?;
    let conn = guard.lock();
    let mut stmt = conn.prepare(
        "SELECT conversation_id FROM usage_accum WHERE llm_rounds > 0",
    )?;
    let ids: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    let mut n = 0usize;
    for cid in ids {
        finalize_accum_if_nonzero(&conn, &cid)?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE usage_accum SET
               prompt_tokens=0, completion_tokens=0, thinking_tokens=0, total_tokens=0,
               llm_rounds=0, period_start=NULL, period_end=NULL, updated_at=?2
             WHERE conversation_id = ?1",
            params![cid, now],
        )?;
        n += 1;
    }
    if n > 0 {
        log::info!("token_usage_store: finalized {n} stale conversation accum(s)");
    }
    Ok(n)
}

fn read_all_pending(conn: &Connection) -> Result<Vec<PendingTokenUsageReport>> {
    let mut stmt = conn.prepare(
        "SELECT request_id, prompt_tokens, completion_tokens, thinking_tokens, total_tokens,
                llm_rounds, model_name, period_start, period_end
         FROM usage_pending ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(PendingTokenUsageReport {
            request_id: row.get(0)?,
            prompt_tokens: row.get(1)?,
            completion_tokens: row.get(2)?,
            thinking_tokens: row.get(3)?,
            total_tokens: row.get(4)?,
            assistant_rounds: Some(row.get::<_, u32>(5)?),
            model_name: row.get(6)?,
            period_start: row.get(7)?,
            period_end: row.get(8)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn delete_pending(conn: &Connection, request_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM usage_pending WHERE request_id = ?1",
        params![request_id],
    )?;
    Ok(())
}

pub async fn flush_pending_reports(
    auth: &crate::platform_auth::PlatformAuthManager,
) -> Result<usize> {
    let pending = {
        let guard = connection()?;
        let conn = guard.lock();
        read_all_pending(&conn)?
    };
    if pending.is_empty() {
        return Ok(0);
    }
    let mut sent = 0usize;
    for row in pending {
        let body = serde_json::json!({
            "prompt_tokens": row.prompt_tokens,
            "completion_tokens": row.completion_tokens,
            "thinking_tokens": row.thinking_tokens,
            "total_tokens": row.total_tokens,
            "model_name": row.model_name,
            "request_id": row.request_id,
            "assistant_rounds": row.assistant_rounds,
            "period_start": row.period_start,
            "period_end": row.period_end,
        });
        match auth.report_token_usage(body).await {
            Ok(()) => {
                let guard = connection()?;
                let conn = guard.lock();
                delete_pending(&conn, &row.request_id)?;
                sent += 1;
            }
            Err(e) => {
                log::warn!(
                    "token_usage_store: report failed request_id={}: {e}",
                    row.request_id
                );
            }
        }
    }
    if sent > 0 {
        log::info!("token_usage_store: flushed {sent} report(s)");
    }
    Ok(sent)
}

