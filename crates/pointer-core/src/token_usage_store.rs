//! SQLite-backed LLM token usage per run_chat × agent_instance_id; multipart upload to platform.

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
use uuid::Uuid;

use crate::agent_instance_scope::AgentInstanceScope;
use crate::llm_token_stats::LlmUsageSnapshot;
use crate::models::ChatMessage;
use crate::storage::app_data_dir;

const DB_FILE: &str = "token_usage.db";
const LEGACY_PENDING_FILE: &str = "token_usage_pending.jsonl";
const LEGACY_INSTANCE_PREFIX: &str = "legacy:";

const REPORT_STATUS_ACCUMULATING: &str = "accumulating";
const REPORT_STATUS_PENDING: &str = "pending";
const REPORT_STATUS_SENT: &str = "sent";

static DB: OnceLock<Mutex<Connection>> = OnceLock::new();

pub fn request_id_for_run(run_id: &str, agent_instance_id: &str) -> String {
    format!("run:{run_id}:{agent_instance_id}")
}

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
    if table_exists(conn, "usage_accum")? && !table_has_column(conn, "usage_accum", "run_id")? {
        migrate_usage_accum_v2(conn)?;
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

fn synthetic_run_id_from_request_id(request_id: &str) -> String {
    if let Some(rest) = request_id.strip_prefix("run:") {
        if let Some(idx) = rest.rfind(':') {
            let prefix = &rest[..idx];
            if !prefix.is_empty() {
                return prefix.to_string();
            }
        }
    }
    format!("migrated:{}", Uuid::new_v4())
}

fn migrate_usage_accum_v2(conn: &Connection) -> Result<()> {
    log::info!("token_usage_store: migrating usage_accum to run_id + report_status (drop usage_pending)");
    conn.execute_batch(
        "ALTER TABLE usage_accum RENAME TO usage_accum_old;
         CREATE TABLE usage_accum (
           run_id TEXT NOT NULL,
           conversation_id TEXT NOT NULL,
           agent_instance_id TEXT NOT NULL,
           agent_role_id TEXT,
           prompt_tokens INTEGER NOT NULL DEFAULT 0,
           completion_tokens INTEGER NOT NULL DEFAULT 0,
           thinking_tokens INTEGER NOT NULL DEFAULT 0,
           total_tokens INTEGER NOT NULL DEFAULT 0,
           llm_rounds INTEGER NOT NULL DEFAULT 0,
           model_name TEXT,
           model_totals_json TEXT NOT NULL DEFAULT '{}',
           period_start TEXT,
           period_end TEXT,
           report_status TEXT NOT NULL DEFAULT 'accumulating',
           request_id TEXT NOT NULL,
           history_archive_path TEXT,
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL,
           PRIMARY KEY (run_id, agent_instance_id)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS idx_usage_accum_request_id ON usage_accum(request_id);",
    )?;

    conn.execute(
        "INSERT INTO usage_accum (
           run_id, conversation_id, agent_instance_id, agent_role_id,
           prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
           model_name, model_totals_json, period_start, period_end,
           report_status, request_id, history_archive_path, created_at, updated_at
         )
         SELECT
           'legacy:' || conversation_id || ':' || agent_instance_id,
           conversation_id,
           agent_instance_id,
           agent_role_id,
           prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
           model_name,
           COALESCE(model_totals_json, '{}'),
           period_start, period_end,
           CASE WHEN total_tokens > 0 THEN 'pending' ELSE 'sent' END,
           'run:legacy:' || conversation_id || ':' || agent_instance_id,
           NULL,
           updated_at,
           updated_at
         FROM usage_accum_old
         WHERE llm_rounds > 0 OR total_tokens > 0",
        [],
    )?;

    if table_exists(conn, "usage_pending")? {
        let mut stmt = conn.prepare(
            "SELECT request_id, conversation_id, agent_instance_id, agent_role_id,
                    prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
                    model_name, model_totals_json, period_start, period_end,
                    history_archive_path, created_at
             FROM usage_pending",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, u32>(4)?,
                    row.get::<_, u32>(5)?,
                    row.get::<_, u32>(6)?,
                    row.get::<_, u32>(7)?,
                    row.get::<_, u32>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, String>(14)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for row in rows {
            let run_id = synthetic_run_id_from_request_id(&row.0);
            let model_totals = row.10.unwrap_or_else(|| "{}".into());
            conn.execute(
                "INSERT OR IGNORE INTO usage_accum (
                   run_id, conversation_id, agent_instance_id, agent_role_id,
                   prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
                   model_name, model_totals_json, period_start, period_end,
                   report_status, request_id, history_archive_path, created_at, updated_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,'pending',?14,?15,?16,?16)",
                params![
                    run_id,
                    row.1,
                    row.2,
                    row.3,
                    row.4,
                    row.5,
                    row.6,
                    row.7,
                    row.8,
                    row.9,
                    model_totals,
                    row.11,
                    row.12,
                    row.0,
                    row.13,
                    row.14,
                ],
            )?;
        }
        conn.execute("DROP TABLE usage_pending", [])?;
    }

    conn.execute("DROP TABLE usage_accum_old", [])?;
    log::info!("token_usage_store: usage_accum v2 migration complete");
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
    if !table_has_column(conn, "usage_accum", "run_id")? {
        return Ok(());
    }
    let path = app_data_dir()?.join(LEGACY_PENDING_FILE);
    if !path.exists() {
        return Ok(());
    }
    let f = File::open(&path)?;
    let mut migrated = 0u32;
    let legacy_conversation_id = "legacy";
    let legacy_instance = format!("{LEGACY_INSTANCE_PREFIX}{legacy_conversation_id}");
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
        let run_id = synthetic_run_id_from_request_id(&row.request_id);
        let n = conn.execute(
            "INSERT OR IGNORE INTO usage_accum (
               run_id, conversation_id, agent_instance_id, agent_role_id,
               prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
               model_name, model_totals_json, period_start, period_end,
               report_status, request_id, history_archive_path, created_at, updated_at
             ) VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8, ?9, '{}', ?10, ?11, 'pending', ?12, NULL, ?13, ?13)",
            params![
                run_id,
                legacy_conversation_id,
                legacy_instance,
                row.prompt_tokens,
                row.completion_tokens,
                row.thinking_tokens,
                row.total_tokens,
                rounds,
                row.model_name,
                period_start,
                period_end,
                row.request_id,
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

struct ReportRow {
    run_id: String,
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

/// Ensure accum row exists for this agent instance in the current run.
pub fn ensure_accum(scope: &AgentInstanceScope) -> Result<()> {
    let guard = connection()?;
    let conn = guard.lock();
    let now = Utc::now().to_rfc3339();
    let request_id = request_id_for_run(&scope.run_id, &scope.agent_instance_id);
    conn.execute(
        "INSERT OR IGNORE INTO usage_accum (
           run_id, conversation_id, agent_instance_id, agent_role_id,
           prompt_tokens, completion_tokens, thinking_tokens, total_tokens,
           llm_rounds, model_name, model_totals_json, period_start, period_end,
           report_status, request_id, history_archive_path, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, 0, 0, 0, 0, 0, NULL, '{}', NULL, NULL, ?5, ?6, NULL, ?7, ?7)",
        params![
            scope.run_id,
            scope.conversation_id,
            scope.agent_instance_id,
            scope.agent_role_id,
            REPORT_STATUS_ACCUMULATING,
            request_id,
            now,
        ],
    )?;
    Ok(())
}

fn maybe_write_history_archive(
    conversation_id: &str,
    agent_instance_id: &str,
    agent_role_id: &str,
    history: &[ChatMessage],
) -> Result<Option<String>> {
    use crate::conversation_snapshot::{build_snapshot_json, write_snapshot_zip};

    let snapshot = build_snapshot_json(conversation_id, agent_instance_id, agent_role_id, history);
    let empty = snapshot["messages"]
        .as_array()
        .map(|messages| messages.is_empty())
        .unwrap_or(true);
    if empty {
        return Ok(None);
    }
    let dir = app_data_dir()?.join("token_usage_archives");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{conversation_id}_{agent_instance_id}.zip"));
    write_snapshot_zip(&path, &snapshot)?;
    Ok(Some(path.to_string_lossy().into_owned()))
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
            "SELECT model_totals_json FROM usage_accum
             WHERE run_id = ?1 AND agent_instance_id = ?2",
            params![scope.run_id, scope.agent_instance_id],
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
             WHERE run_id = ?1 AND agent_instance_id = ?2 AND report_status = ?10",
            params![
                scope.run_id,
                scope.agent_instance_id,
                u.prompt_tokens,
                u.completion_tokens,
                u.reasoning_tokens,
                u.total_tokens,
                model,
                totals,
                now,
                REPORT_STATUS_ACCUMULATING,
            ],
        )?;
    } else {
        conn.execute(
            "UPDATE usage_accum SET
               llm_rounds = llm_rounds + 1,
               period_start = COALESCE(period_start, ?3),
               period_end = ?3,
               updated_at = ?3
             WHERE run_id = ?1 AND agent_instance_id = ?2 AND report_status = ?4",
            params![
                scope.run_id,
                scope.agent_instance_id,
                now,
                REPORT_STATUS_ACCUMULATING,
            ],
        )?;
    }
    Ok(())
}

/// Mark this run's accumulating rows as pending and attach optional history archives.
pub fn finalize_run(run_id: &str, conversation_id: &str, history: &[ChatMessage]) -> Result<()> {
    let guard = connection()?;
    let conn = guard.lock();
    let now = Utc::now().to_rfc3339();

    struct Row {
        agent_instance_id: String,
        agent_role_id: Option<String>,
    }

    let mut stmt = conn.prepare(
        "SELECT agent_instance_id, agent_role_id
         FROM usage_accum
         WHERE run_id = ?1 AND report_status = ?2 AND total_tokens > 0",
    )?;
    let rows = stmt
        .query_map(params![run_id, REPORT_STATUS_ACCUMULATING], |row| {
            Ok(Row {
                agent_instance_id: row.get(0)?,
                agent_role_id: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    for row in rows {
        let role_id = row.agent_role_id.as_deref().unwrap_or("unknown");
        let archive_path = maybe_write_history_archive(
            conversation_id,
            &row.agent_instance_id,
            role_id,
            history,
        )?;
        conn.execute(
            "UPDATE usage_accum SET
               report_status = ?3,
               history_archive_path = COALESCE(?4, history_archive_path),
               updated_at = ?5
             WHERE run_id = ?1 AND agent_instance_id = ?2",
            params![
                run_id,
                row.agent_instance_id,
                REPORT_STATUS_PENDING,
                archive_path,
                now,
            ],
        )?;
    }
    Ok(())
}

/// Promote interrupted accumulating rows to pending (no history archive).
pub fn finalize_all_stale_accum() -> Result<usize> {
    let guard = connection()?;
    let conn = guard.lock();
    let now = Utc::now().to_rfc3339();
    let n = conn.execute(
        "UPDATE usage_accum SET report_status = ?1, updated_at = ?2
         WHERE report_status = ?3 AND total_tokens > 0",
        params![REPORT_STATUS_PENDING, now, REPORT_STATUS_ACCUMULATING],
    )?;
    if n > 0 {
        log::info!("token_usage_store: promoted {n} stale accumulating row(s) to pending");
    }
    Ok(n)
}

fn read_unsent_reports(conn: &Connection) -> Result<Vec<ReportRow>> {
    let mut stmt = conn.prepare(
        "SELECT run_id, request_id, conversation_id, agent_instance_id, agent_role_id,
                prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
                model_name, model_totals_json, period_start, period_end, history_archive_path
         FROM usage_accum
         WHERE report_status = ?1
         ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![REPORT_STATUS_PENDING], |row| {
        Ok(ReportRow {
            run_id: row.get(0)?,
            request_id: row.get(1)?,
            conversation_id: row.get(2)?,
            agent_instance_id: row.get(3)?,
            agent_role_id: row.get(4)?,
            prompt_tokens: row.get(5)?,
            completion_tokens: row.get(6)?,
            thinking_tokens: row.get(7)?,
            total_tokens: row.get(8)?,
            llm_rounds: row.get(9)?,
            model_name: row.get(10)?,
            model_totals_json: row.get(11)?,
            period_start: row.get(12)?,
            period_end: row.get(13)?,
            history_archive_path: row.get(14)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn mark_report_sent(conn: &Connection, request_id: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE usage_accum SET report_status = ?2, updated_at = ?3 WHERE request_id = ?1",
        params![request_id, REPORT_STATUS_SENT, now],
    )?;
    Ok(())
}

fn build_report_metadata(row: &ReportRow, platform_agent_id: Option<String>) -> serde_json::Value {
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

pub async fn flush_unsent_reports(
    auth: &crate::platform_auth::PlatformAuthManager,
) -> Result<usize> {
    let pending = {
        let guard = connection()?;
        let conn = guard.lock();
        read_unsent_reports(&conn)?
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
                mark_report_sent(&conn, &row.request_id)?;
                sent += 1;
            }
            Err(e) => {
                log::warn!(
                    "token_usage_store: report failed run_id={} request_id={} agent_instance_id={}: {e}",
                    row.run_id,
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

/// Backward-compatible alias.
pub async fn flush_pending_reports(
    auth: &crate::platform_auth::PlatformAuthManager,
) -> Result<usize> {
    flush_unsent_reports(auth).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_migrated_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        migrate_schema(&conn).expect("migrate schema");
        conn
    }

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

        assert!(table_has_column(&conn, "usage_accum", "run_id").expect("run_id column"));
        assert!(!table_exists(&conn, "usage_pending").expect("pending dropped"));
    }

    #[test]
    fn report_metadata_omits_null_optionals_and_includes_conversation_id() {
        let row = ReportRow {
            run_id: "run-1".into(),
            request_id: "run:run-1:inst1".into(),
            conversation_id: "conv1".into(),
            agent_instance_id: "inst1".into(),
            agent_role_id: None,
            prompt_tokens: 1,
            completion_tokens: 2,
            thinking_tokens: 0,
            total_tokens: 3,
            llm_rounds: 2,
            model_name: None,
            model_totals_json: None,
            period_start: None,
            period_end: None,
            history_archive_path: None,
        };
        let metadata = build_report_metadata(&row, Some("agent-1".into()));
        assert_eq!(metadata["conversation_id"], "conv1");
        assert_eq!(metadata["platform_agent_id"], "agent-1");
        assert_eq!(metadata["assistant_rounds"], 2);
        assert!(metadata.get("agent_role_id").is_none());
        assert!(metadata.get("model_name").is_none());
    }

    #[test]
    fn finalize_marks_pending_without_clearing_tokens() {
        let conn = open_migrated_db();
        let run_id = "run-finalize";
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO usage_accum (
               run_id, conversation_id, agent_instance_id, agent_role_id,
               prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
               model_name, model_totals_json, period_start, period_end,
               report_status, request_id, created_at, updated_at
             ) VALUES (?1, 'conv', 'inst', 'coder', 10, 5, 0, 15, 2, 'qwen', '{\"qwen\":15}',
                       ?2, ?2, 'accumulating', 'run:run-finalize:inst', ?2, ?2)",
            params![run_id, now],
        )
        .expect("seed row");

        conn.execute(
            "UPDATE usage_accum SET report_status = 'pending', updated_at = ?2
             WHERE run_id = ?1 AND report_status = 'accumulating' AND total_tokens > 0",
            params![run_id, now],
        )
        .expect("finalize");

        let status: String = conn
            .query_row(
                "SELECT report_status FROM usage_accum WHERE run_id = ?1",
                params![run_id],
                |r| r.get(0),
            )
            .expect("status");
        assert_eq!(status, REPORT_STATUS_PENDING);

        let total: u32 = conn
            .query_row(
                "SELECT total_tokens FROM usage_accum WHERE run_id = ?1",
                params![run_id],
                |r| r.get(0),
            )
            .expect("total");
        assert_eq!(total, 15);
    }

    #[test]
    fn request_id_uses_run_id_and_instance() {
        assert_eq!(
            request_id_for_run("abc-run", "inst-1"),
            "run:abc-run:inst-1"
        );
    }

    #[test]
    fn stale_accum_promoted_to_pending() {
        let conn = open_migrated_db();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO usage_accum (
               run_id, conversation_id, agent_instance_id, agent_role_id,
               prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
               model_name, model_totals_json, report_status, request_id, created_at, updated_at
             ) VALUES ('stale-run', 'conv', 'inst', 'coder', 1, 1, 0, 2, 1, NULL, '{}',
                       'accumulating', 'run:stale-run:inst', ?1, ?1)",
            params![now],
        )
        .expect("insert");

        let n = conn
            .execute(
                "UPDATE usage_accum SET report_status = 'pending', updated_at = ?1
                 WHERE report_status = 'accumulating' AND total_tokens > 0",
                params![now],
            )
            .expect("promote");
        assert_eq!(n, 1);
    }
}
