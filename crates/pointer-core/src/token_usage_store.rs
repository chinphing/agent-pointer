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
use sha2::{Digest, Sha256};
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

const BILLING_MODE_TOKENS: &str = "tokens";
const PLATFORM_AGENT_INSTANCE_NAMESPACE: Uuid = Uuid::from_u128(0x6ba7b8109dad11d1_80b4_00c04fd430c8);

/// Partner API accepts request_id up to 128 chars; keep stable hash when longer.
pub fn platform_request_id(raw: &str) -> String {
    if raw.len() <= 128 {
        return raw.to_string();
    }
    let digest = Sha256::digest(raw.as_bytes());
    let hex = digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    format!("run:hash:{}", &hex[..32])
}

/// Partner API agent_instance_id max 36 (UUID). Hash non-UUID / legacy ids deterministically.
pub fn platform_agent_instance_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.len() <= 36 && Uuid::parse_str(trimmed).is_ok() {
        return trimmed.to_string();
    }
    Uuid::new_v5(&PLATFORM_AGENT_INSTANCE_NAMESPACE, trimmed.as_bytes()).to_string()
}

pub fn count_unsent_reports() -> Result<usize> {
    let guard = connection()?;
    let conn = guard.lock();
    let n: i64 = conn.query_row(
        "SELECT COUNT(1) FROM usage_accum WHERE report_status = ?1",
        params![REPORT_STATUS_PENDING],
        |row| row.get(0),
    )?;
    Ok(n as usize)
}

/// Billing unit metadata for platform upload (tokens / per-image / per-sec).
#[derive(Debug, Clone, Copy)]
pub struct UsageBillingMeta {
    pub billing_mode: &'static str,
    pub unit_count: u32,
}

static DB: OnceLock<Mutex<Connection>> = OnceLock::new();

pub fn request_id_for_run(run_id: &str, agent_instance_id: &str, model_name: &str) -> String {
    format!("run:{run_id}:{agent_instance_id}:{model_name}")
}

fn usage_model_key(model_name: Option<&str>) -> String {
    model_name
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or("unknown")
        .to_string()
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
    if table_exists(conn, "usage_accum")? && table_has_column(conn, "usage_accum", "model_totals_json")? {
        migrate_usage_accum_v3(conn)?;
    }
    if table_exists(conn, "usage_accum")? && !table_has_column(conn, "usage_accum", "billing_mode")? {
        migrate_usage_accum_v4(conn)?;
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

struct UsageAccumV2Row {
    run_id: String,
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
    report_status: String,
    #[allow(dead_code)]
    request_id: String,
    history_archive_path: Option<String>,
    created_at: String,
    updated_at: String,
}

fn split_tokens_by_ratio(total: u32, model_total: u64, json_total: u64) -> u32 {
    if json_total == 0 || total == 0 {
        return 0;
    }
    ((u64::from(total) * model_total + json_total / 2) / json_total).min(u64::from(u32::MAX)) as u32
}

fn migrate_usage_accum_v3(conn: &Connection) -> Result<()> {
    log::info!("token_usage_store: migrating usage_accum to per-model rows (drop model_totals_json)");
    conn.execute_batch(
        "ALTER TABLE usage_accum RENAME TO usage_accum_v2_old;
         CREATE TABLE usage_accum (
           run_id TEXT NOT NULL,
           conversation_id TEXT NOT NULL,
           agent_instance_id TEXT NOT NULL,
           model_name TEXT NOT NULL DEFAULT 'unknown',
           agent_role_id TEXT,
           prompt_tokens INTEGER NOT NULL DEFAULT 0,
           completion_tokens INTEGER NOT NULL DEFAULT 0,
           thinking_tokens INTEGER NOT NULL DEFAULT 0,
           total_tokens INTEGER NOT NULL DEFAULT 0,
           llm_rounds INTEGER NOT NULL DEFAULT 0,
           period_start TEXT,
           period_end TEXT,
           report_status TEXT NOT NULL DEFAULT 'accumulating',
           request_id TEXT NOT NULL,
           history_archive_path TEXT,
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL,
           PRIMARY KEY (run_id, agent_instance_id, model_name)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS idx_usage_accum_request_id ON usage_accum(request_id);",
    )?;

    let mut stmt = conn.prepare(
        "SELECT run_id, conversation_id, agent_instance_id, agent_role_id,
                prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
                model_name, model_totals_json, period_start, period_end,
                report_status, request_id, history_archive_path, created_at, updated_at
         FROM usage_accum_v2_old",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(UsageAccumV2Row {
                run_id: row.get(0)?,
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
                report_status: row.get(13)?,
                request_id: row.get(14)?,
                history_archive_path: row.get(15)?,
                created_at: row.get(16)?,
                updated_at: row.get(17)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    for row in rows {
        let model_totals: HashMap<String, u64> = row
            .model_totals_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        let models: Vec<(String, u64)> = if model_totals.is_empty() {
            vec![(
                usage_model_key(row.model_name.as_deref()),
                u64::from(row.total_tokens),
            )]
        } else {
            model_totals.into_iter().collect()
        };
        let json_total: u64 = models.iter().map(|(_, t)| t).sum();
        let multi_model = models.len() > 1;
        let mut assigned_prompt = 0u32;
        let mut assigned_completion = 0u32;
        let mut assigned_thinking = 0u32;
        let mut assigned_rounds = 0u32;
        for (idx, (model, model_total)) in models.iter().enumerate() {
            let is_last = idx + 1 == models.len();
            let (prompt, completion, thinking, rounds) = if multi_model {
                if is_last {
                    (
                        row.prompt_tokens.saturating_sub(assigned_prompt),
                        row.completion_tokens.saturating_sub(assigned_completion),
                        row.thinking_tokens.saturating_sub(assigned_thinking),
                        row.llm_rounds.saturating_sub(assigned_rounds),
                    )
                } else {
                    let prompt = split_tokens_by_ratio(row.prompt_tokens, *model_total, json_total);
                    let completion =
                        split_tokens_by_ratio(row.completion_tokens, *model_total, json_total);
                    let thinking =
                        split_tokens_by_ratio(row.thinking_tokens, *model_total, json_total);
                    let rounds = split_tokens_by_ratio(row.llm_rounds, *model_total, json_total);
                    assigned_prompt = assigned_prompt.saturating_add(prompt);
                    assigned_completion = assigned_completion.saturating_add(completion);
                    assigned_thinking = assigned_thinking.saturating_add(thinking);
                    assigned_rounds = assigned_rounds.saturating_add(rounds);
                    (prompt, completion, thinking, rounds)
                }
            } else {
                (
                    row.prompt_tokens,
                    row.completion_tokens,
                    row.thinking_tokens,
                    row.llm_rounds,
                )
            };
            let total = if multi_model {
                (*model_total).min(u64::from(u32::MAX)) as u32
            } else {
                row.total_tokens
            };
            let request_id = request_id_for_run(&row.run_id, &row.agent_instance_id, model);
            let archive = if is_last {
                row.history_archive_path.clone()
            } else {
                None
            };
            conn.execute(
                "INSERT INTO usage_accum (
                   run_id, conversation_id, agent_instance_id, model_name, agent_role_id,
                   prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
                   period_start, period_end, report_status, request_id, history_archive_path,
                   created_at, updated_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                params![
                    row.run_id,
                    row.conversation_id,
                    row.agent_instance_id,
                    model,
                    row.agent_role_id,
                    prompt,
                    completion,
                    thinking,
                    total,
                    rounds,
                    row.period_start,
                    row.period_end,
                    row.report_status,
                    request_id,
                    archive,
                    row.created_at,
                    row.updated_at,
                ],
            )?;
        }
    }

    conn.execute("DROP TABLE usage_accum_v2_old", [])?;
    log::info!("token_usage_store: usage_accum v3 migration complete");
    Ok(())
}

fn migrate_usage_accum_v4(conn: &Connection) -> Result<()> {
    log::info!("token_usage_store: migrating usage_accum to billing_mode + unit_count");
    conn.execute_batch(
        "ALTER TABLE usage_accum ADD COLUMN billing_mode TEXT NOT NULL DEFAULT 'tokens';
         ALTER TABLE usage_accum ADD COLUMN unit_count INTEGER NOT NULL DEFAULT 0;",
    )?;
    log::info!("token_usage_store: usage_accum v4 migration complete");
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
        let model = usage_model_key(row.model_name.as_deref());
        let request_id = request_id_for_run(&run_id, &legacy_instance, &model);
        let n = conn.execute(
            "INSERT OR IGNORE INTO usage_accum (
               run_id, conversation_id, agent_instance_id, model_name, agent_role_id,
               prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
               period_start, period_end,
               report_status, request_id, history_archive_path, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'pending', ?12, NULL, ?13, ?13)",
            params![
                run_id,
                legacy_conversation_id,
                legacy_instance,
                model,
                row.prompt_tokens,
                row.completion_tokens,
                row.thinking_tokens,
                row.total_tokens,
                rounds,
                period_start,
                period_end,
                request_id,
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
    model_name: String,
    prompt_tokens: u32,
    completion_tokens: u32,
    thinking_tokens: u32,
    total_tokens: u32,
    llm_rounds: u32,
    billing_mode: String,
    unit_count: u32,
    period_start: Option<String>,
    period_end: Option<String>,
    history_archive_path: Option<String>,
}

fn ensure_model_accum_row(scope: &AgentInstanceScope, model: &str) -> Result<()> {
    let guard = connection()?;
    let conn = guard.lock();
    let now = Utc::now().to_rfc3339();
    let request_id = request_id_for_run(&scope.run_id, &scope.agent_instance_id, model);
    conn.execute(
        "INSERT OR IGNORE INTO usage_accum (
           run_id, conversation_id, agent_instance_id, model_name, agent_role_id,
           prompt_tokens, completion_tokens, thinking_tokens, total_tokens,
           llm_rounds, billing_mode, unit_count, period_start, period_end,
           report_status, request_id, history_archive_path, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, 0, 0, 0, ?6, 0, NULL, NULL, ?7, ?8, NULL, ?9, ?9)",
        params![
            scope.run_id,
            scope.conversation_id,
            scope.agent_instance_id,
            model,
            scope.agent_role_id,
            BILLING_MODE_TOKENS,
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

/// Persist one LLM round for an agent instance (one row per model).
pub fn record_round(
    scope: &AgentInstanceScope,
    usage: Option<&LlmUsageSnapshot>,
    model_name: Option<&str>,
    billing: Option<&UsageBillingMeta>,
) -> Result<()> {
    let model = usage_model_key(model_name);
    ensure_model_accum_row(scope, &model)?;
    let guard = connection()?;
    let conn = guard.lock();
    let now = Utc::now().to_rfc3339();
    let billing_mode = billing.map(|b| b.billing_mode).unwrap_or(BILLING_MODE_TOKENS);
    let unit_delta = billing.map(|b| b.unit_count).unwrap_or(0);
    if let Some(u) = usage {
        conn.execute(
            "UPDATE usage_accum SET
               prompt_tokens = prompt_tokens + ?4,
               completion_tokens = completion_tokens + ?5,
               thinking_tokens = thinking_tokens + ?6,
               total_tokens = total_tokens + ?7,
               llm_rounds = llm_rounds + 1,
               billing_mode = CASE WHEN ?8 != ?9 THEN ?8 ELSE billing_mode END,
               unit_count = unit_count + ?10,
               period_start = COALESCE(period_start, ?11),
               period_end = ?11,
               updated_at = ?11
             WHERE run_id = ?1 AND agent_instance_id = ?2 AND model_name = ?3
               AND report_status = ?12",
            params![
                scope.run_id,
                scope.agent_instance_id,
                model,
                u.prompt_tokens,
                u.completion_tokens,
                u.reasoning_tokens,
                u.total_tokens,
                billing_mode,
                BILLING_MODE_TOKENS,
                unit_delta,
                now,
                REPORT_STATUS_ACCUMULATING,
            ],
        )?;
    } else {
        conn.execute(
            "UPDATE usage_accum SET
               llm_rounds = llm_rounds + 1,
               billing_mode = CASE WHEN ?4 != ?5 THEN ?4 ELSE billing_mode END,
               unit_count = unit_count + ?6,
               period_start = COALESCE(period_start, ?7),
               period_end = ?7,
               updated_at = ?7
             WHERE run_id = ?1 AND agent_instance_id = ?2 AND model_name = ?3
               AND report_status = ?8",
            params![
                scope.run_id,
                scope.agent_instance_id,
                model,
                billing_mode,
                BILLING_MODE_TOKENS,
                unit_delta,
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
        "SELECT DISTINCT agent_instance_id, agent_role_id
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
               updated_at = ?4
             WHERE run_id = ?1 AND agent_instance_id = ?2 AND report_status = ?5",
            params![
                run_id,
                row.agent_instance_id,
                REPORT_STATUS_PENDING,
                now,
                REPORT_STATUS_ACCUMULATING,
            ],
        )?;
        if let Some(path) = archive_path {
            conn.execute(
                "UPDATE usage_accum SET history_archive_path = ?3
                 WHERE run_id = ?1 AND agent_instance_id = ?2
                   AND model_name = (
                     SELECT model_name FROM usage_accum
                     WHERE run_id = ?1 AND agent_instance_id = ?2
                     ORDER BY total_tokens DESC, model_name ASC
                     LIMIT 1
                   )",
                params![run_id, row.agent_instance_id, path],
            )?;
        }
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
                model_name, prompt_tokens, completion_tokens, thinking_tokens, total_tokens,
                llm_rounds, billing_mode, unit_count, period_start, period_end, history_archive_path
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
            model_name: row.get(5)?,
            prompt_tokens: row.get(6)?,
            completion_tokens: row.get(7)?,
            thinking_tokens: row.get(8)?,
            total_tokens: row.get(9)?,
            llm_rounds: row.get(10)?,
            billing_mode: row.get(11)?,
            unit_count: row.get(12)?,
            period_start: row.get(13)?,
            period_end: row.get(14)?,
            history_archive_path: row.get(15)?,
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
    let mut metadata = json!({
        "request_id": platform_request_id(&row.request_id),
        "conversation_id": row.conversation_id,
        "agent_instance_id": platform_agent_instance_id(&row.agent_instance_id),
        "model_name": row.model_name,
        "prompt_tokens": row.prompt_tokens,
        "completion_tokens": row.completion_tokens,
        "thinking_tokens": row.thinking_tokens,
        "total_tokens": row.total_tokens,
        "assistant_rounds": row.llm_rounds,
        "billing_mode": row.billing_mode,
    });
    if row.unit_count > 0 {
        metadata["unit_count"] = json!(row.unit_count);
    }
    if let Some(id) = platform_agent_id.filter(|id| !id.is_empty()) {
        metadata["platform_agent_id"] = json!(id);
    }
    if let Some(role) = row.agent_role_id.as_deref().filter(|role| !role.is_empty()) {
        metadata["agent_role_id"] = json!(role);
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

const ENV_USAGE_REPORT_ENABLED: &str = "POINTER_USAGE_REPORT_ENABLED";

fn parse_bool_env(key: &str) -> Option<bool> {
    match std::env::var(key) {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            Some(t == "1" || t == "true" || t == "yes" || t == "on")
        }
        Err(_) => None,
    }
}

/// Whether unsent token-usage reports should be flushed to the platform API.
/// Standalone defaults to false; platform mode defaults to true.
pub fn usage_report_enabled() -> bool {
    parse_bool_env(ENV_USAGE_REPORT_ENABLED)
        .unwrap_or_else(|| !crate::deployment_mode::is_standalone())
}

pub async fn flush_unsent_reports(
    auth: &crate::platform_auth::PlatformAuthManager,
) -> Result<usize> {
    if !usage_report_enabled() {
        return Ok(0);
    }
    if !auth.session_view().logged_in {
        let pending_n = count_unsent_reports().unwrap_or(0);
        if pending_n > 0 {
            log::warn!(
                "token_usage_store: skip flush — not logged in; {pending_n} pending report(s) waiting"
            );
        }
        return Ok(0);
    }
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
                        log::debug!(
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
    fn report_metadata_includes_model_and_token_fields() {
        let row = ReportRow {
            run_id: "run-1".into(),
            request_id: "run:run-1:inst1:qwen".into(),
            conversation_id: "conv1".into(),
            agent_instance_id: "inst1".into(),
            agent_role_id: None,
            model_name: "qwen".into(),
            prompt_tokens: 1,
            completion_tokens: 2,
            thinking_tokens: 0,
            total_tokens: 3,
            llm_rounds: 2,
            billing_mode: "tokens".into(),
            unit_count: 0,
            period_start: None,
            period_end: None,
            history_archive_path: None,
        };
        let metadata = build_report_metadata(&row, Some("agent-1".into()));
        assert_eq!(metadata["conversation_id"], "conv1");
        assert_eq!(metadata["platform_agent_id"], "agent-1");
        assert_eq!(metadata["model_name"], "qwen");
        assert_eq!(metadata["prompt_tokens"], 1);
        assert_eq!(metadata["total_tokens"], 3);
        assert_eq!(metadata["assistant_rounds"], 2);
        assert_eq!(metadata["billing_mode"], "tokens");
        assert!(metadata.get("unit_count").is_none());
        assert!(metadata.get("agent_role_id").is_none());
        assert!(metadata.get("model_totals").is_none());
    }

    #[test]
    fn platform_request_id_hashes_when_too_long() {
        let raw = format!(
            "run:{}:{}:{}",
            "a".repeat(36),
            "b".repeat(36),
            "c".repeat(80)
        );
        assert!(raw.len() > 128);
        let clipped = platform_request_id(&raw);
        assert!(clipped.len() <= 128);
        assert_eq!(clipped, platform_request_id(&raw));
        assert!(clipped.starts_with("run:hash:"));
    }

    #[test]
    fn platform_agent_instance_id_hashes_legacy_values() {
        let legacy = "legacy:conversation-id-with-colons";
        let out = platform_agent_instance_id(legacy);
        assert_eq!(out.len(), 36);
        assert!(Uuid::parse_str(&out).is_ok());
        assert_eq!(out, platform_agent_instance_id(legacy));
    }

    #[test]
    fn report_metadata_clamps_legacy_agent_instance_id() {
        let row = ReportRow {
            run_id: "run-1".into(),
            request_id: "run:run-1:legacy:conv:qwen".into(),
            conversation_id: "conv1".into(),
            agent_instance_id: "legacy:conversation-abc".into(),
            agent_role_id: Some("general".into()),
            model_name: "qwen".into(),
            prompt_tokens: 1,
            completion_tokens: 2,
            thinking_tokens: 0,
            total_tokens: 3,
            llm_rounds: 1,
            billing_mode: "tokens".into(),
            unit_count: 0,
            period_start: None,
            period_end: None,
            history_archive_path: None,
        };
        let metadata = build_report_metadata(&row, None);
        let id = metadata["agent_instance_id"].as_str().unwrap();
        assert_eq!(id.len(), 36);
        assert!(Uuid::parse_str(id).is_ok());
    }

    #[test]
    fn report_metadata_includes_unit_count_for_per_image() {
        let row = ReportRow {
            run_id: "run-1".into(),
            request_id: "run:run-1:inst1:wan@per-image".into(),
            conversation_id: "conv1".into(),
            agent_instance_id: "inst1".into(),
            agent_role_id: Some("media-image-generate".into()),
            model_name: "wan2.7-image-pro@per-image".into(),
            prompt_tokens: 0,
            completion_tokens: 20_000,
            thinking_tokens: 0,
            total_tokens: 20_000,
            llm_rounds: 1,
            billing_mode: "per-image".into(),
            unit_count: 2,
            period_start: None,
            period_end: None,
            history_archive_path: None,
        };
        let metadata = build_report_metadata(&row, None);
        assert_eq!(metadata["billing_mode"], "per-image");
        assert_eq!(metadata["unit_count"], 2);
    }

    #[test]
    fn finalize_marks_pending_without_clearing_tokens() {
        let conn = open_migrated_db();
        let run_id = "run-finalize";
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO usage_accum (
               run_id, conversation_id, agent_instance_id, model_name, agent_role_id,
               prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
               period_start, period_end,
               report_status, request_id, created_at, updated_at
             ) VALUES (?1, 'conv', 'inst', 'qwen', 'coder', 10, 5, 0, 15, 2,
                       ?2, ?2, 'accumulating', 'run:run-finalize:inst:qwen', ?2, ?2)",
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
    fn request_id_uses_run_id_instance_and_model() {
        assert_eq!(
            request_id_for_run("abc-run", "inst-1", "qwen"),
            "run:abc-run:inst-1:qwen"
        );
    }

    #[test]
    fn migrates_v2_model_totals_json_into_per_model_rows() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE usage_accum (
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
             );",
        )
        .expect("v2 schema");
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO usage_accum (
               run_id, conversation_id, agent_instance_id, agent_role_id,
               prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
               model_name, model_totals_json, period_start, period_end,
               report_status, request_id, created_at, updated_at
             ) VALUES ('run-v3', 'conv', 'inst', 'coder', 100, 50, 10, 150, 3, 'qwen',
                       '{\"qwen\":100,\"gpt-4\":50}', ?1, ?1, 'accumulating',
                       'run:run-v3:inst', ?1, ?1)",
            params![now],
        )
        .expect("seed v2 row");

        migrate_usage_accum_v3(&conn).expect("v3 migrate");

        assert!(
            !table_has_column(&conn, "usage_accum", "model_totals_json").expect("column check")
        );
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM usage_accum WHERE run_id = 'run-v3'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(count, 2);

        let qwen_total: u32 = conn
            .query_row(
                "SELECT total_tokens FROM usage_accum
                 WHERE run_id = 'run-v3' AND model_name = 'qwen'",
                [],
                |r| r.get(0),
            )
            .expect("qwen");
        assert_eq!(qwen_total, 100);
    }

    #[test]
    fn stale_accum_promoted_to_pending() {
        let conn = open_migrated_db();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO usage_accum (
               run_id, conversation_id, agent_instance_id, model_name, agent_role_id,
               prompt_tokens, completion_tokens, thinking_tokens, total_tokens, llm_rounds,
               report_status, request_id, created_at, updated_at
             ) VALUES ('stale-run', 'conv', 'inst', 'unknown', 'coder', 1, 1, 0, 2, 1,
                       'accumulating', 'run:stale-run:inst:unknown', ?1, ?1)",
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
