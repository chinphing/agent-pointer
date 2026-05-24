//! SQLite-backed LLM token usage per agent_instance_id; multipart zip upload to platform.

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::agent_instance_scope::AgentInstanceScope;
use crate::conversation_snapshot::{build_snapshot_json, write_snapshot_zip};
use crate::llm_token_stats::LlmUsageSnapshot;
use crate::models::ChatMessage;
use crate::storage::app_data_dir;

const DB_FILE: &str = "token_usage.db";
const ARCHIVE_SUBDIR: &str = "token_usage_archives";
const LEGACY_PENDING_FILE: &str = "token_usage_pending.jsonl";
const LEGACY_INSTANCE_PREFIX: &str = "legacy:";

static DB: OnceLock<Mutex<Connection>> = OnceLock::new();

fn db_path() -> Result<PathBuf> {
    Ok(app_data_dir()?.join(DB_FILE))
}

fn archive_dir() -> Result<PathBuf> {
    let d = app_data_dir()?.join(ARCHIVE_SUBDIR);
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

fn connection() -> Result<&'static Mutex<Connection>> {
    if DB.get().is_none() {
        let path = db_path()?;
        let conn = Connection::open(&path)
            .with_context(|| format!("open token usage db {}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;",
        )?;
        migrate_schema(&conn)?;
        let _ = DB.set(Mutex::new(conn));
    }
    Ok(DB.get().expect("token usage db initialized"))
}

fn migrate_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS usage_accum (
           conversation_id TEXT NOT NULL,
           agent_instance_id TEXT NOT NULL,
           agent_role_id TEXT,
           prompt_tokens INTEGER NOT NULL DEFAULT 0,
           completion_tokens INTEGER NOT NULL DEFAULT 0,
           thinking_tokens INTEGER NOT NULL DEFAULT 0,
           total_tokens INTEGER NOT NULL DEFAULT 0,
           llm_rounds INTEGER NOT NULL DEFAULT 0,
           model_name TEXT,
           model_totals_json TEXT,
           period_start TEXT,
           period_end TEXT,
           updated_at TEXT NOT NULL,
           PRIMARY KEY (conversation_id, agent_instance_id)
         );
         CREATE TABLE IF NOT EXISTS usage_pending (
           request_id TEXT PRIMARY KEY NOT NULL,
           conversation_id TEXT NOT NULL,
           agent_instance_id TEXT NOT NULL,
           agent_role_id TEXT,
           prompt_tokens INTEGER NOT NULL,
           completion_tokens INTEGER NOT NULL,
           thinking_tokens INTEGER NOT NULL,
           total_tokens INTEGER NOT NULL,
           llm_rounds INTEGER NOT NULL,
           model_name TEXT,
           model_totals_json TEXT,
           period_start TEXT,
           period_end TEXT,
           history_archive_path TEXT,
           created_at TEXT NOT NULL
         );",
    )?;
    if table_exists(conn, "usage_accum")? && !table_has_column(conn, "usage_accum", "agent_instance_id")? {
        migrate_usage_accum_v1(conn)?;
    }
    if table_exists(conn, "usage_pending")? && !table_has_column(conn, "usage_pending", "agent_instance_id")? {
        migrate_usage_pending_v1(conn)?;
    }
    migrate_legacy_jsonl(conn)?;
    Ok(())
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        params![table],
        |row| row.get(0),
    )?;
    Ok(n > 0)
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let sql = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn migrate_usage_accum_v1(conn: &Connection) -> Result<()> {
    log::info!("token_usage_store: migrating usage_accum schema to per agent_instance_id");
    conn.execute_batch(
        "ALTER TABLE usage_accum RENAME TO usage_accum_v1;
         CREATE TABLE usage_accum (
           conversation_id TEXT NOT NULL,
           agent_instance_id TEXT NOT NULL,
           agent_role_id TEXT,
           prompt_tokens INTEGER NOT NULL DEFAULT 0,
           completion_tokens INTEGER NOT NULL DEFAULT 0,
           thinking_tokens INTEGER NOT NULL DEFAULT 0,
           total_tokens INTEGER NOT NULL DEFAULT 0,
           llm_rounds INTEGER NOT NULL DEFAULT 0,
           model_name TEXT,
           model_totals_json TEXT,
           period_start TEXT,
           period_end TEXT,
           updated_at TEXT NOT NULL,
           PRIMARY KEY (conversation_id, agent_instance_id)
         );
         INSERT INTO usage_accum (
           conversation_id, agent_instance_id, agent_role_id,
           prompt_tokens, completion_tokens, thinking_tokens, total_tokens,
           llm_rounds, model_name, model_totals_json, period_start, period_end, updated_at
         )
         SELECT
           conversation_id,
           'legacy:' || conversation_id,
           NULL,
           prompt_tokens, completion_tokens, thinking_tokens, total_tokens,
           llm_rounds, model_name, '{}', period_start, period_end, updated_at
         FROM usage_accum_v1;
         DROP TABLE usage_accum_v1;",
    )?;
    Ok(())
}

fn migrate_usage_pending_v1(conn: &Connection) -> Result<()> {
    log::info!("token_usage_store: migrating usage_pending schema to per agent_instance_id");
    conn.execute_batch(
        "ALTER TABLE usage_pending RENAME TO usage_pending_v1;
         CREATE TABLE usage_pending (
           request_id TEXT PRIMARY KEY NOT NULL,
           conversation_id TEXT NOT NULL,
           agent_instance_id TEXT NOT NULL,
           agent_role_id TEXT,
           prompt_tokens INTEGER NOT NULL,
           completion_tokens INTEGER NOT NULL,
           thinking_tokens INTEGER NOT NULL,
           total_tokens INTEGER NOT NULL,
           llm_rounds INTEGER NOT NULL,
           model_name TEXT,
           model_totals_json TEXT,
           period_start TEXT,
           period_end TEXT,
           history_archive_path TEXT,
           created_at TEXT NOT NULL
         );
         INSERT INTO usage_pending (
           request_id, conversation_id, agent_instance_id, agent_role_id,
           prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
           model_name, model_totals_json, period_start, period_end, history_archive_path, created_at
         )
         SELECT
           request_id,
           conversation_id,
           'legacy:' || conversation_id,
           NULL,
           prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
           model_name, '{}', period_start, period_end, NULL, created_at
         FROM usage_pending_v1;
         DROP TABLE usage_pending_v1;",
    )?;
    Ok(())
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

fn migrate_legacy_jsonl(conn: &Connection) -> Result<()> {
    if !table_has_column(conn, "usage_pending", "agent_instance_id")? {
        return Ok(());
    }
    let path = app_data_dir()?.join(LEGACY_PENDING_FILE);
    if !path.exists() {
        return Ok(());
    }
    let f = File::open(&path)?;
    let mut migrated = 0u32;
    let legacy_conversation_id = "legacy";
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
               request_id, conversation_id, agent_instance_id, agent_role_id,
               prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
               model_name, model_totals_json, period_start, period_end, history_archive_path, created_at
             ) VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8, ?9, '{}', ?10, ?11, NULL, ?12)",
            params![
                row.request_id,
                legacy_conversation_id,
                format!("{LEGACY_INSTANCE_PREFIX}{legacy_conversation_id}"),
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

struct AccumRow {
    agent_instance_id: String,
    agent_role_id: Option<String>,
    prompt_tokens: u32,
    completion_tokens: u32,
    thinking_tokens: u32,
    total_tokens: u32,
    llm_rounds: u32,
    model_name: Option<String>,
    model_totals_json: Option<String>,
    period_start: Option<String>,
    period_end: Option<String>,
}

struct PendingRow {
    request_id: String,
    conversation_id: String,
    agent_instance_id: String,
    agent_role_id: Option<String>,
    prompt_tokens: u32,
    completion_tokens: u32,
    thinking_tokens: u32,
    total_tokens: u32,
    llm_rounds: u32,
    model_name: Option<String>,
    model_totals_json: Option<String>,
    period_start: Option<String>,
    period_end: Option<String>,
    history_archive_path: Option<String>,
}

fn merge_model_totals_json(existing: Option<&str>, model: Option<&str>, delta: u32) -> String {
    let mut map: HashMap<String, u64> = existing
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    if delta > 0 {
        let key = model.unwrap_or("unknown").to_string();
        *map.entry(key).or_insert(0) += u64::from(delta);
    }
    serde_json::to_string(&map).unwrap_or_else(|_| "{}".into())
}

/// Ensure accum row exists for this agent instance.
pub fn ensure_accum(scope: &AgentInstanceScope) -> Result<()> {
    let guard = connection()?;
    let conn = guard.lock();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR IGNORE INTO usage_accum (
           conversation_id, agent_instance_id, agent_role_id,
           prompt_tokens, completion_tokens, thinking_tokens, total_tokens,
           llm_rounds, model_name, model_totals_json, period_start, period_end, updated_at
         ) VALUES (?1, ?2, ?3, 0, 0, 0, 0, 0, NULL, '{}', NULL, NULL, ?4)",
        params![
            scope.conversation_id,
            scope.agent_instance_id,
            scope.agent_role_id,
            now,
        ],
    )?;
    Ok(())
}

/// Persist one LLM round for an agent instance.
pub fn record_round(
    scope: &AgentInstanceScope,
    usage: Option<&LlmUsageSnapshot>,
    model_name: Option<&str>,
) -> Result<()> {
    ensure_accum(scope)?;
    let guard = connection()?;
    let conn = guard.lock();
    let now = Utc::now().to_rfc3339();
    let existing_totals: Option<String> = conn
        .query_row(
            "SELECT model_totals_json FROM usage_accum WHERE conversation_id = ?1 AND agent_instance_id = ?2",
            params![scope.conversation_id, scope.agent_instance_id],
            |r| r.get(0),
        )
        .ok();
    if let Some(u) = usage {
        let totals = merge_model_totals_json(existing_totals.as_deref(), model_name, u.total_tokens);
        let model = model_name.map(str::to_string);
        conn.execute(
            "UPDATE usage_accum SET
               prompt_tokens = prompt_tokens + ?3,
               completion_tokens = completion_tokens + ?4,
               thinking_tokens = thinking_tokens + ?5,
               total_tokens = total_tokens + ?6,
               llm_rounds = llm_rounds + 1,
               model_name = COALESCE(?7, model_name),
               model_totals_json = ?8,
               period_start = COALESCE(period_start, ?9),
               period_end = ?9,
               updated_at = ?9
             WHERE conversation_id = ?1 AND agent_instance_id = ?2",
            params![
                scope.conversation_id,
                scope.agent_instance_id,
                u.prompt_tokens,
                u.completion_tokens,
                u.reasoning_tokens,
                u.total_tokens,
                model,
                totals,
                now,
            ],
        )?;
    } else {
        conn.execute(
            "UPDATE usage_accum SET
               llm_rounds = llm_rounds + 1,
               period_start = COALESCE(period_start, ?3),
               period_end = ?3,
               updated_at = ?3
             WHERE conversation_id = ?1 AND agent_instance_id = ?2",
            params![scope.conversation_id, scope.agent_instance_id, now],
        )?;
    }
    Ok(())
}

fn read_accums_for_conversation(conn: &Connection, conversation_id: &str) -> Result<Vec<AccumRow>> {
    let mut stmt = conn.prepare(
        "SELECT agent_instance_id, agent_role_id, prompt_tokens, completion_tokens, thinking_tokens,
                total_tokens, llm_rounds, model_name, model_totals_json, period_start, period_end
         FROM usage_accum WHERE conversation_id = ?1 AND llm_rounds > 0",
    )?;
    let rows = stmt.query_map(params![conversation_id], |row| {
        Ok(AccumRow {
            agent_instance_id: row.get(0)?,
            agent_role_id: row.get(1)?,
            prompt_tokens: row.get(2)?,
            completion_tokens: row.get(3)?,
            thinking_tokens: row.get(4)?,
            total_tokens: row.get(5)?,
            llm_rounds: row.get(6)?,
            model_name: row.get(7)?,
            model_totals_json: row.get(8)?,
            period_start: row.get(9)?,
            period_end: row.get(10)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn insert_pending(
    conn: &Connection,
    conversation_id: &str,
    accum: &AccumRow,
    history: &[ChatMessage],
) -> Result<()> {
    let thinking = accum.thinking_tokens;
    let completion = accum.completion_tokens.saturating_sub(thinking);
    let agent_role = accum.agent_role_id.clone().unwrap_or_default();
    let request_id = format!(
        "run:{conversation_id}:{}",
        accum.agent_instance_id
    );
    let zip_path = archive_dir()?.join(format!("{}.zip", accum.agent_instance_id));
    let snapshot = build_snapshot_json(
        conversation_id,
        &accum.agent_instance_id,
        &agent_role,
        history,
    );
    write_snapshot_zip(&zip_path, &snapshot)?;
    let now = Utc::now().to_rfc3339();
    let period_start = accum.period_start.clone().unwrap_or_else(|| now.clone());
    let period_end = accum.period_end.clone().unwrap_or(now);
    conn.execute(
        "INSERT INTO usage_pending (
           request_id, conversation_id, agent_instance_id, agent_role_id,
           prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
           model_name, model_totals_json, period_start, period_end, history_archive_path, created_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        params![
            request_id,
            conversation_id,
            accum.agent_instance_id,
            accum.agent_role_id,
            accum.prompt_tokens,
            completion,
            thinking,
            accum.total_tokens,
            accum.llm_rounds,
            accum.model_name,
            accum.model_totals_json,
            period_start,
            period_end,
            zip_path.to_string_lossy().to_string(),
            Utc::now().to_rfc3339(),
        ],
    )?;
    log::info!(
        "token_usage_store: enqueued request_id={} agent_instance_id={} agent_role_id={} total_tokens={}",
        request_id,
        accum.agent_instance_id,
        agent_role,
        accum.total_tokens
    );
    Ok(())
}

/// Finalize all agent instances for a conversation into pending queue (with zip snapshots).
pub fn finalize_run(conversation_id: &str, history: &[ChatMessage]) -> Result<()> {
    let guard = connection()?;
    let conn = guard.lock();
    let accums = read_accums_for_conversation(&conn, conversation_id)?;
    for accum in &accums {
        insert_pending(&conn, conversation_id, accum, history)?;
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

pub fn finalize_all_stale_accum() -> Result<usize> {
    let guard = connection()?;
    let conn = guard.lock();
    let mut stmt = conn.prepare("SELECT DISTINCT conversation_id FROM usage_accum WHERE llm_rounds > 0")?;
    let ids: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    let mut n = 0usize;
    for cid in ids {
        let accums = read_accums_for_conversation(&conn, &cid)?;
        for accum in &accums {
            let empty: &[ChatMessage] = &[];
            insert_pending(&conn, &cid, accum, empty)?;
            n += 1;
        }
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE usage_accum SET prompt_tokens=0, completion_tokens=0, thinking_tokens=0,
             total_tokens=0, llm_rounds=0, period_start=NULL, period_end=NULL, updated_at=?2
             WHERE conversation_id = ?1",
            params![cid, now],
        )?;
    }
    if n > 0 {
        log::info!("token_usage_store: finalized {n} stale accum row(s)");
    }
    Ok(n)
}

fn read_all_pending(conn: &Connection) -> Result<Vec<PendingRow>> {
    let mut stmt = conn.prepare(
        "SELECT request_id, conversation_id, agent_instance_id, agent_role_id,
                prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
                model_name, model_totals_json, period_start, period_end, history_archive_path
         FROM usage_pending ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(PendingRow {
            request_id: row.get(0)?,
            conversation_id: row.get(1)?,
            agent_instance_id: row.get(2)?,
            agent_role_id: row.get(3)?,
            prompt_tokens: row.get(4)?,
            completion_tokens: row.get(5)?,
            thinking_tokens: row.get(6)?,
            total_tokens: row.get(7)?,
            llm_rounds: row.get(8)?,
            model_name: row.get(9)?,
            model_totals_json: row.get(10)?,
            period_start: row.get(11)?,
            period_end: row.get(12)?,
            history_archive_path: row.get(13)?,
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

fn build_report_metadata(row: &PendingRow, platform_agent_id: Option<String>) -> serde_json::Value {
    let model_totals: Option<serde_json::Value> = row
        .model_totals_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let mut metadata = json!({
        "request_id": row.request_id,
        "conversation_id": row.conversation_id,
        "agent_instance_id": row.agent_instance_id,
        "prompt_tokens": row.prompt_tokens,
        "completion_tokens": row.completion_tokens,
        "thinking_tokens": row.thinking_tokens,
        "total_tokens": row.total_tokens,
        "assistant_rounds": row.llm_rounds,
    });
    if let Some(id) = platform_agent_id.filter(|id| !id.is_empty()) {
        metadata["platform_agent_id"] = json!(id);
    }
    if let Some(role) = row.agent_role_id.as_deref().filter(|role| !role.is_empty()) {
        metadata["agent_role_id"] = json!(role);
    }
    if let Some(model) = row.model_name.as_deref().filter(|model| !model.is_empty()) {
        metadata["model_name"] = json!(model);
    }
    if let Some(totals) = model_totals.filter(|v| !v.is_null()) {
        metadata["model_totals"] = totals;
    }
    if let Some(start) = row.period_start.as_deref().filter(|v| !v.is_empty()) {
        metadata["period_start"] = json!(start);
    }
    if let Some(end) = row.period_end.as_deref().filter(|v| !v.is_empty()) {
        metadata["period_end"] = json!(end);
    }
    metadata
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportDelivery {
    Multipart,
    JsonOnlyFallback,
}

async fn send_pending_report(
    auth: &crate::platform_auth::PlatformAuthManager,
    metadata: &serde_json::Value,
    zip_path: Option<&std::path::Path>,
) -> Result<ReportDelivery> {
    let request_id = metadata
        .get("request_id")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    match auth
        .report_token_usage_multipart(metadata, zip_path)
        .await
    {
        Ok(()) => Ok(ReportDelivery::Multipart),
        Err(multipart_err) => {
            log::warn!(
                "token_usage_store: multipart failed request_id={request_id}: {multipart_err}; retrying json-only"
            );
            auth.report_token_usage(metadata.clone())
                .await
                .map_err(|json_err| {
                    anyhow!("multipart: {multipart_err}; json-only: {json_err}")
                })?;
            Ok(ReportDelivery::JsonOnlyFallback)
        }
    }
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
    let platform_agent_id = auth.platform_agent_id();
    let mut sent = 0usize;
    for row in pending {
        let metadata = build_report_metadata(&row, platform_agent_id.clone());
        let zip_path = row
            .history_archive_path
            .as_deref()
            .filter(|p| !p.is_empty())
            .map(PathBuf::from);
        match send_pending_report(auth, &metadata, zip_path.as_deref()).await {
            Ok(delivery) => {
                let rid = row.request_id.as_str();
                match delivery {
                    ReportDelivery::Multipart => {
                        log::info!(
                            "token_usage_store: report ok request_id={rid} (multipart+archive)"
                        );
                    }
                    ReportDelivery::JsonOnlyFallback => {
                        log::info!(
                            "token_usage_store: multipart with archive failed, json-only retry succeeded request_id={rid}"
                        );
                    }
                }
                if let Some(p) = zip_path.as_ref() {
                    let _ = std::fs::remove_file(p);
                }
                let guard = connection()?;
                let conn = guard.lock();
                delete_pending(&conn, &row.request_id)?;
                sent += 1;
            }
            Err(e) => {
                log::warn!(
                    "token_usage_store: report failed request_id={} agent_instance_id={}: {e}",
                    row.request_id,
                    row.agent_instance_id
                );
            }
        }
    }
    if sent > 0 {
        log::info!("token_usage_store: flushed {sent} report(s)");
    }
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_v1_token_usage_schema() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE usage_accum (
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
             CREATE TABLE usage_pending (
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
             );
             INSERT INTO usage_accum VALUES ('conv1', 10, 20, 0, 30, 2, 'gpt', NULL, NULL, '2026-01-01T00:00:00Z');",
        )
        .expect("seed v1 schema");

        migrate_schema(&conn).expect("migrate schema");

        assert!(table_has_column(&conn, "usage_accum", "agent_instance_id").expect("accum column"));
        assert!(table_has_column(&conn, "usage_pending", "agent_instance_id").expect("pending column"));
        let agent_instance_id: String = conn
            .query_row(
                "SELECT agent_instance_id FROM usage_accum WHERE conversation_id = 'conv1'",
                [],
                |row| row.get(0),
            )
            .expect("migrated accum row");
        assert_eq!(agent_instance_id, "legacy:conv1");
    }

    #[test]
    fn report_metadata_omits_null_optionals_and_includes_conversation_id() {
        let row = PendingRow {
            request_id: "run:conv1:inst1".into(),
            conversation_id: "conv1".into(),
            agent_instance_id: "inst1".into(),
            agent_role_id: None,
            prompt_tokens: 1,
            completion_tokens: 2,
            thinking_tokens: 0,
            total_tokens: 3,
            llm_rounds: 1,
            model_name: None,
            model_totals_json: None,
            period_start: None,
            period_end: None,
            history_archive_path: None,
        };
        let metadata = build_report_metadata(&row, Some("agent-1".into()));
        assert_eq!(metadata["conversation_id"], "conv1");
        assert_eq!(metadata["platform_agent_id"], "agent-1");
        assert!(metadata.get("agent_role_id").is_none());
        assert!(metadata.get("model_name").is_none());
    }
}
