//! `webhook_sources` table: per-ingress-source session state (daily rollover or per-delivery).

use anyhow::Result;
use chrono::{DateTime, Local, TimeZone};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::cron_jobs::{daily_reset_at_ms, CRON_SESSION_RESET_AT_HOUR};
use crate::webhook_config::{per_delivery_session_id, webhook_session_key, webhook_session_title};

/// How inbound webhooks map to conversation sessions for one `:src`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookSessionMode {
    /// One session per local calendar day (`webhook:{src}:{yyyymmdd}`), 04:00 rollover.
    Daily,
    /// One session per delivery id (`webhook:{src}:{delivery_id}`).
    #[default]
    PerDelivery,
}

impl WebhookSessionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::PerDelivery => "per_delivery",
        }
    }

    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_lowercase().as_str() {
            "daily" => Ok(Self::Daily),
            "per_delivery" | "per-delivery" | "perdelivery" => Ok(Self::PerDelivery),
            other if other.is_empty() => Ok(Self::default()),
            _ => anyhow::bail!("invalid webhook session_mode (use daily or per_delivery)"),
        }
    }

    pub fn from_db(raw: Option<&str>) -> Self {
        raw.map(Self::parse)
            .transpose()
            .ok()
            .flatten()
            .unwrap_or_default()
    }
}

/// Row tracking the active webhook session for one `:src`.
#[derive(Debug, Clone)]
pub struct WebhookSourceRecord {
    pub src: String,
    /// Active session id (`webhook:{src}:{yyyymmdd}` or `webhook:{src}:{delivery_id}`).
    pub current_session_id: Option<String>,
    pub last_ingress_at_ms: Option<i64>,
    /// Optional custom auth header (e.g. `X-Codeup-Token`); NULL = default Bearer + X-Pointer-Token.
    pub auth_header_name: Option<String>,
    /// `per_delivery` (default) or `daily`.
    pub session_mode: WebhookSessionMode,
    pub created_at_ms: i64,
}

pub fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS webhook_sources (
           src TEXT PRIMARY KEY,
           current_session_id TEXT,
           last_ingress_at_ms INTEGER,
           auth_header_name TEXT,
           session_mode TEXT NOT NULL DEFAULT 'per_delivery',
           created_at_ms INTEGER NOT NULL
         );",
    )?;
    Ok(())
}

pub fn get(conn: &Connection, src: &str) -> Result<Option<WebhookSourceRecord>> {
    Ok(conn
        .query_row(
            "SELECT src, current_session_id, last_ingress_at_ms, auth_header_name, session_mode, created_at_ms
         FROM webhook_sources WHERE src = ?1",
            params![src],
            map_row,
        )
        .optional()?)
}

pub fn ensure_row(conn: &Connection, src: &str, now_ms: i64) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO webhook_sources (src, current_session_id, last_ingress_at_ms, auth_header_name, session_mode, created_at_ms)
         VALUES (?1, NULL, NULL, NULL, 'per_delivery', ?2)",
        params![src, now_ms],
    )?;
    Ok(())
}

pub fn set_current_session_id(conn: &Connection, src: &str, session_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE webhook_sources SET current_session_id = ?2 WHERE src = ?1",
        params![src, session_id],
    )?;
    Ok(())
}

pub fn touch_ingress(conn: &Connection, src: &str, now_ms: i64, session_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE webhook_sources
         SET last_ingress_at_ms = ?2, current_session_id = ?3
         WHERE src = ?1",
        params![src, now_ms, session_id],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, src: &str) -> Result<bool> {
    let n = conn.execute("DELETE FROM webhook_sources WHERE src = ?1", params![src])?;
    Ok(n > 0)
}

pub fn set_auth_header_name(
    conn: &Connection,
    src: &str,
    auth_header_name: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE webhook_sources SET auth_header_name = ?2 WHERE src = ?1",
        params![src, auth_header_name],
    )?;
    Ok(())
}

pub fn session_mode(conn: &Connection, src: &str) -> Result<WebhookSessionMode> {
    Ok(WebhookSessionMode::from_db(
        conn.query_row(
            "SELECT session_mode FROM webhook_sources WHERE src = ?1",
            params![src],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .as_deref(),
    ))
}

pub fn set_session_mode(conn: &Connection, src: &str, mode: WebhookSessionMode) -> Result<()> {
    conn.execute(
        "UPDATE webhook_sources SET session_mode = ?2 WHERE src = ?1",
        params![src, mode.as_str()],
    )?;
    log::info!(
        "webhook_sources: session_mode set src={src} mode={}",
        mode.as_str()
    );
    Ok(())
}

pub fn auth_header_name(conn: &Connection, src: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT auth_header_name FROM webhook_sources WHERE src = ?1",
            params![src],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WebhookSourceRecord> {
    Ok(WebhookSourceRecord {
        src: row.get(0)?,
        current_session_id: row.get(1)?,
        last_ingress_at_ms: row.get(2)?,
        auth_header_name: row.get(3)?,
        session_mode: WebhookSessionMode::from_db(row.get::<_, Option<String>>(4)?.as_deref()),
        created_at_ms: row.get(5)?,
    })
}

/// Active webhook session id at `now`: `webhook:{src}:{yyyymmdd}` (04:00 local rollover).
pub fn current_webhook_session_id<Z: TimeZone>(src: &str, now: &DateTime<Z>) -> String {
    let boundary_ms = daily_reset_at_ms(now, CRON_SESSION_RESET_AT_HOUR);
    let boundary_local = Local.timestamp_millis_opt(boundary_ms).unwrap();
    format!("webhook:{}:{}", src, boundary_local.format("%Y%m%d"))
}

/// Resolve which session id an ingress should write to.
pub fn resolve_ingress_session_id(
    conn: &Connection,
    src: &str,
    now: &DateTime<Local>,
    delivery_id: Option<&str>,
) -> Result<String> {
    let now_ms = now.timestamp_millis();
    ensure_row(conn, src, now_ms)?;
    let mode = session_mode(conn, src)?;
    match mode {
        WebhookSessionMode::Daily => resolve_daily_ingress_session(conn, src, now),
        WebhookSessionMode::PerDelivery => {
            let delivery = delivery_id
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| crate::webhook_config::new_delivery_id());
            resolve_per_delivery_ingress_session(conn, src, &delivery, now)
        }
    }
}

fn resolve_daily_ingress_session(
    conn: &Connection,
    src: &str,
    now: &DateTime<Local>,
) -> Result<String> {
    let expected = current_webhook_session_id(src, now);
    let now_ms = now.timestamp_millis();
    let record = get(conn, src)?;
    let current = record.as_ref().and_then(|r| r.current_session_id.clone());

    let session_id = expected.clone();
    if current.as_deref() != Some(session_id.as_str()) {
        if current
            .as_deref()
            .is_some_and(|cur| cur == webhook_session_key(src))
        {
            log::info!(
                "webhook_sources: migrated legacy session to dated src={src} session={session_id}"
            );
        }
        set_current_session_id(conn, src, &session_id)?;
        let title = webhook_session_title(src);
        super::write::ensure_conversation_row_with_title(conn, &session_id, Some(&title))?;
        log::info!(
            "webhook_sources: session {} src={} session={}",
            if current.is_none() {
                "first-ingress"
            } else {
                "rollover"
            },
            src,
            session_id
        );
    } else {
        let title = webhook_session_title(src);
        super::write::ensure_conversation_row_with_title(conn, &session_id, Some(&title))?;
    }
    touch_ingress(conn, src, now_ms, &session_id)?;
    Ok(session_id)
}

fn resolve_per_delivery_ingress_session(
    conn: &Connection,
    src: &str,
    delivery_id: &str,
    now: &DateTime<Local>,
) -> Result<String> {
    let session_id = per_delivery_session_id(src, delivery_id);
    let now_ms = now.timestamp_millis();
    let title = webhook_session_title(src);
    super::write::ensure_conversation_row_with_title(conn, &session_id, Some(&title))?;
    touch_ingress(conn, src, now_ms, &session_id)?;
    log::info!(
        "webhook_sources: per-delivery ingress src={src} delivery={delivery_id} session={session_id}"
    );
    Ok(session_id)
}

/// When upload supplies an explicit webhook session id, keep `webhook_sources` in sync.
pub fn adopt_upload_session(
    conn: &Connection,
    src: &str,
    conversation_id: &str,
    now: &DateTime<Local>,
) -> Result<()> {
    if !crate::webhook_config::conversation_id_matches_webhook_src(conversation_id, src) {
        return Ok(());
    }
    let now_ms = now.timestamp_millis();
    ensure_row(conn, src, now_ms)?;
    set_current_session_id(conn, src, conversation_id)?;
    let title = crate::webhook_config::webhook_session_title(src);
    super::write::ensure_conversation_row_with_title(conn, conversation_id, Some(&title))?;
    touch_ingress(conn, src, now_ms, conversation_id)?;
    log::info!("webhook_sources: adopted upload session src={src} session={conversation_id}");
    Ok(())
}

/// Session id for the automation UI "查看会话" entry (never returns legacy `webhook:{src}`).
pub fn resolve_view_session_id(record: Option<&WebhookSourceRecord>, src: &str) -> Option<String> {
    let mode = record
        .map(|r| r.session_mode)
        .unwrap_or(WebhookSessionMode::PerDelivery);
    let legacy = webhook_session_key(src);
    if let Some(r) = record {
        if let Some(ref id) = r.current_session_id {
            let id = id.trim();
            if !id.is_empty() && id != legacy {
                return Some(id.to_string());
            }
        }
        if mode == WebhookSessionMode::PerDelivery {
            return None;
        }
        if let Some(ms) = r.last_ingress_at_ms {
            let dt = Local
                .timestamp_millis_opt(ms)
                .single()
                .unwrap_or_else(Local::now);
            return Some(current_webhook_session_id(src, &dt));
        }
    }
    match mode {
        WebhookSessionMode::PerDelivery => None,
        WebhookSessionMode::Daily => Some(current_webhook_session_id(src, &Local::now())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS conversations (
               id TEXT PRIMARY KEY,
               title TEXT NOT NULL,
               created_at_ms INTEGER NOT NULL,
               updated_at_ms INTEGER NOT NULL,
               message_count INTEGER NOT NULL DEFAULT 0,
               preview TEXT NOT NULL DEFAULT '',
               skill_ids_json TEXT NOT NULL DEFAULT '[]',
               tool_rounds_used INTEGER NOT NULL DEFAULT 0,
               tool_rounds_used_supervisor INTEGER NOT NULL DEFAULT 0,
               computer_monitor_id TEXT,
               workspace_root TEXT NOT NULL DEFAULT '',
               workspace_user_set INTEGER NOT NULL DEFAULT 0,
               workspace_inherit_disabled INTEGER NOT NULL DEFAULT 0,
               lead_agent_id TEXT NOT NULL DEFAULT 'general',
               agent_mode TEXT NOT NULL DEFAULT 'single',
               im_session_epoch INTEGER NOT NULL DEFAULT 0,
               im_active_conversation_id TEXT,
               im_last_interaction_at_ms INTEGER NOT NULL DEFAULT 0,
               session_user_id TEXT NOT NULL DEFAULT ''
             );",
        )
        .unwrap();
        ensure_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn current_webhook_session_id_matches_cron_reset_window() {
        let src = "github";
        let now = Local
            .with_ymd_and_hms(2026, 6, 28, 10, 0, 0)
            .single()
            .unwrap();
        let webhook_id = current_webhook_session_id(src, &now);
        let cron_id = crate::conversation_store::cron_jobs::current_cron_session_id("x", &now);
        assert_eq!(webhook_id, "webhook:github:20260628");
        assert_eq!(cron_id, "cron:x:20260628");
    }

    #[test]
    fn first_ingress_default_is_per_delivery() {
        let conn = mem();
        let now = Local
            .with_ymd_and_hms(2026, 6, 28, 10, 0, 0)
            .single()
            .unwrap();
        let id1 = resolve_ingress_session_id(&conn, "github", &now, Some("del-1")).unwrap();
        let id2 = resolve_ingress_session_id(&conn, "github", &now, Some("del-2")).unwrap();
        assert_eq!(id1, "webhook:github:del-1");
        assert_eq!(id2, "webhook:github:del-2");
    }

    #[test]
    fn first_ingress_uses_dated_session_when_daily_mode() {
        let conn = mem();
        ensure_row(&conn, "github", 1).unwrap();
        set_session_mode(&conn, "github", WebhookSessionMode::Daily).unwrap();
        let now = Local
            .with_ymd_and_hms(2026, 6, 28, 10, 0, 0)
            .single()
            .unwrap();
        let id = resolve_ingress_session_id(&conn, "github", &now, None).unwrap();
        assert_eq!(id, "webhook:github:20260628");
    }

    #[test]
    fn legacy_db_state_migrates_to_dated_on_ingress() {
        let conn = mem();
        ensure_row(&conn, "github", 1).unwrap();
        set_session_mode(&conn, "github", WebhookSessionMode::Daily).unwrap();
        let day1 = Local
            .with_ymd_and_hms(2026, 6, 28, 10, 0, 0)
            .single()
            .unwrap();
        set_current_session_id(&conn, "github", "webhook:github").unwrap();
        let id1 = resolve_ingress_session_id(&conn, "github", &day1, None).unwrap();
        assert_eq!(id1, "webhook:github:20260628");

        let day2 = Local
            .with_ymd_and_hms(2026, 6, 29, 10, 0, 0)
            .single()
            .unwrap();
        let id2 = resolve_ingress_session_id(&conn, "github", &day2, None).unwrap();
        assert_eq!(id2, "webhook:github:20260629");
    }

    #[test]
    fn per_delivery_uses_distinct_sessions() {
        let conn = mem();
        ensure_row(&conn, "ci", 1).unwrap();
        set_session_mode(&conn, "ci", WebhookSessionMode::PerDelivery).unwrap();
        let now = Local
            .with_ymd_and_hms(2026, 6, 28, 10, 0, 0)
            .single()
            .unwrap();
        let id1 = resolve_ingress_session_id(&conn, "ci", &now, Some("del-1")).unwrap();
        let id2 = resolve_ingress_session_id(&conn, "ci", &now, Some("del-2")).unwrap();
        assert_eq!(id1, "webhook:ci:del-1");
        assert_eq!(id2, "webhook:ci:del-2");
        assert_eq!(
            get(&conn, "ci")
                .unwrap()
                .unwrap()
                .current_session_id
                .as_deref(),
            Some("webhook:ci:del-2")
        );
    }

    #[test]
    fn auth_header_name_null_when_unset() {
        let conn = mem();
        ensure_row(&conn, "ci", 1).unwrap();
        assert_eq!(auth_header_name(&conn, "ci").unwrap(), None);
    }
}
