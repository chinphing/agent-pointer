//! `cron_jobs` table persistence for the scheduler (Phase 5).
//!
//! One row per scheduled agent run. The scheduler ticker reads enabled rows
//! whose `next_run_at_ms <= now`, dispatches a TriggerRequest via the
//! dispatcher, and updates `last_run_at_ms` / `next_run_at_ms`.
//!
//! Functions take a `&Connection` following the `runs` module convention;
//! [`crate::conversation_store::ConversationStore`] wraps them with connection
//! locking / write retry.

use anyhow::Result;
use chrono::{DateTime, Local, TimeZone};
use rusqlite::{params, Connection, OptionalExtension};

/// Recurring schedule stored as a 6-field cron expression.
pub const SCHEDULE_KIND_CRON: &str = "cron";
/// One-shot delay / absolute time; soft-completes after fire (row kept, disabled).
pub const SCHEDULE_KIND_ONCE: &str = "once";

/// A row in the `cron_jobs` table.
#[derive(Debug, Clone)]
pub struct CronJobRecord {
    pub id: String,
    pub label: String,
    pub cron_expr: String,
    /// `cron` (recurring) or `once` (one-shot soft-complete).
    pub schedule_kind: String,
    /// Original user/model schedule string (for display / audit).
    pub schedule_raw: Option<String>,
    pub conversation_id: String,
    /// Active cron session id (`cron:{job_id}:{yyyymmdd}`, the reset-day label).
    /// `None` until the scheduler first fires the job; advanced on each daily
    /// rollover. The transcript lives under this id in the `messages` table;
    /// prior session ids' transcripts are retained for traceability (aligned
    /// with openclaw's per-sessionId transcript retention).
    pub current_session_id: Option<String>,
    pub prompt_text: String,
    pub agent_mode: Option<String>,
    pub lead_agent_id: Option<String>,
    pub enabled: bool,
    pub last_run_at_ms: Option<i64>,
    pub next_run_at_ms: Option<i64>,
    pub created_at_ms: i64,
    /// Optional Run → IM delivery spec (e.g. "feishu", "feishu:ou_xxx",
    /// "feishu:group:chatid", comma-separated, "all"). `None` / empty means no
    /// IM push after the run. Consumed by `ImDeliverHook` via
    /// `trigger_meta.extra.deliver`.
    pub deliver: Option<String>,
    /// Last IM delivery error for this job (best-effort). Cleared on success.
    pub last_delivery_error: Option<String>,
}

/// Input for creating a new cron job.
///
/// `conversation_id` is **ignored** by [`insert`]: each cron job owns a
/// dedicated, isolated session whose id is derived as `cron:{job_id}` (aligned
/// with the OpenClaw cron-session model). The field is retained only for
/// caller compatibility; new callers may pass an empty string.
pub struct NewCronJob<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub cron_expr: &'a str,
    /// `cron` or `once`. Defaults to recurring when empty.
    pub schedule_kind: &'a str,
    /// Original schedule string (e.g. `30m`, `daily@9:30`).
    pub schedule_raw: Option<&'a str>,
    /// Absolute next fire for one-shot jobs. Ignored for recurring (computed
    /// from `cron_expr`).
    pub next_run_at_ms: Option<i64>,
    pub conversation_id: &'a str,
    pub prompt_text: &'a str,
    pub agent_mode: Option<&'a str>,
    pub lead_agent_id: Option<&'a str>,
    pub enabled: bool,
    /// Optional Run → IM delivery spec. `None` / empty means no IM push.
    /// See [`CronJobRecord::deliver`] for the format.
    pub deliver: Option<&'a str>,
}

/// Derive the stable cron session *key* for a job (`cron:{job_id}`). This is
/// stored in `cron_jobs.conversation_id` as a stable identifier; it is NOT the
/// id under which the transcript lives — see [`current_cron_session_id`].
pub fn cron_session_id(job_id: &str) -> String {
    format!("cron:{job_id}")
}

/// Derive the active cron session id for a job at `now`: the reset-day label
/// `cron:{job_id}:{yyyymmdd}` where the date is the calendar date of the most
/// recent daily reset boundary (`CRON_SESSION_RESET_AT_HOUR` local). All ticks
/// within the same `[D 04:00, D+1 04:00)` window share this id and thus share
/// one transcript; crossing the boundary yields a new id → a fresh transcript,
/// while the prior id's transcript stays on disk (openclaw retention model).
pub fn current_cron_session_id<Z: TimeZone>(job_id: &str, now: &DateTime<Z>) -> String {
    let boundary_ms = daily_reset_at_ms(now, CRON_SESSION_RESET_AT_HOUR);
    // The boundary instant is `CRON_SESSION_RESET_AT_HOUR:00` local; format its
    // calendar date in the local timezone so the label matches the user's day.
    let boundary_local = Local.timestamp_millis_opt(boundary_ms).unwrap();
    format!("cron:{}:{}", job_id, boundary_local.format("%Y%m%d"))
}

/// Compute the next firing time (ms since epoch) for a cron expression after
/// `after`. The cron fields (hour, day, …) are interpreted in the timezone of
/// `after` — pass [`Local::now()`] so "9 点" means 9 o'clock local time, not
/// UTC. Returns `None` if the expression has no future firings (e.g. a
/// past-only year range) or fails to parse.
pub fn next_run_ms<Z: TimeZone>(cron_expr: &str, after: &DateTime<Z>) -> Option<i64> {
    let schedule: cron::Schedule = cron_expr.parse().ok()?;
    schedule
        .after(after)
        .next()
        .map(|dt| dt.timestamp_millis())
}

/// Convenience wrapper around [`next_run_ms`] using the current **local** time,
/// so cron expressions are evaluated in the user's local timezone. Lets callers
/// (e.g. Tauri commands) validate a cron expression without depending on
/// `chrono` directly.
pub fn next_run_ms_now(cron_expr: &str) -> Option<i64> {
    next_run_ms(cron_expr, &Local::now())
}

/// Default local hour at which a cron session's transcript rolls over (OpenClaw
/// reset policy, `daily` mode, `atHour = 4`). A cron session whose first turn
/// (`session_started_at`) is before the most recent `04:00` local starts a
/// fresh transcript; per-job preferences (agent / prompt) live on the cron job
/// row, so they survive the rollover.
pub const CRON_SESSION_RESET_AT_HOUR: u32 = 4;

/// Resolve the most recent daily reset instant (ms since epoch) for `now` in
/// `now`'s timezone at the given local hour. Mirrors openclaw's
/// `resolveDailyResetAtMs`: today at `at_hour:00` local; if `now` is before
/// that, the reset boundary is yesterday's.
pub fn daily_reset_at_ms<Z: TimeZone>(now: &DateTime<Z>, at_hour: u32) -> i64 {
    let at_hour = if at_hour > 23 { 0 } else { at_hour };
    let today = now.date_naive();
    let reset_today = today
        .and_hms_opt(at_hour, 0, 0)
        .unwrap_or_else(|| today.and_hms_opt(0, 0, 0).unwrap());
    let reset_today_local = now
        .timezone()
        .from_local_datetime(&reset_today)
        .single()
        .unwrap_or_else(|| now.clone());
    if *now >= reset_today_local {
        reset_today_local.timestamp_millis()
    } else {
        // Yesterday's reset boundary.
        let reset_yesterday = reset_today - chrono::Duration::days(1);
        now.timezone()
            .from_local_datetime(&reset_yesterday)
            .single()
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(reset_today_local.timestamp_millis())
    }
}

/// Insert a new cron job. Recurring jobs compute `next_run_at_ms` from the
/// cron expression; one-shot jobs use [`NewCronJob::next_run_at_ms`]. Returns
/// whether the row was inserted (false on duplicate id).
pub fn insert(conn: &Connection, job: &NewCronJob<'_>) -> Result<bool> {
    let now = Local::now();
    let kind = if job.schedule_kind.trim() == SCHEDULE_KIND_ONCE {
        SCHEDULE_KIND_ONCE
    } else {
        SCHEDULE_KIND_CRON
    };
    let next = if kind == SCHEDULE_KIND_ONCE {
        job.next_run_at_ms
    } else {
        next_run_ms(job.cron_expr, &now)
    };
    if next.is_none() {
        anyhow::bail!(
            "cron_jobs: cannot insert id={} kind={kind}: missing next_run_at_ms",
            job.id
        );
    }
    // Each cron job owns a dedicated isolated session `cron:{id}`; ignore any
    // caller-supplied conversation_id so the job never binds to a user chat.
    let session_id = cron_session_id(job.id);
    if !job.conversation_id.is_empty() && job.conversation_id != session_id {
        log::warn!(
            "cron_jobs: ignoring caller-supplied conversation_id={} for id={}; using dedicated session {}",
            job.conversation_id,
            job.id,
            session_id
        );
    }
    let schedule_raw = job.schedule_raw.map(str::trim).filter(|s| !s.is_empty());
    let affected = conn.execute(
        "INSERT OR IGNORE INTO cron_jobs
           (id, label, cron_expr, schedule_kind, schedule_raw, conversation_id, prompt_text,
            agent_mode, lead_agent_id, enabled, next_run_at_ms, created_at_ms, deliver)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            job.id,
            job.label,
            job.cron_expr,
            kind,
            schedule_raw,
            session_id,
            job.prompt_text,
            job.agent_mode,
            job.lead_agent_id,
            job.enabled as i32,
            next,
            now.timestamp_millis(),
            job.deliver,
        ],
    )?;
    if affected == 0 {
        log::warn!("cron_jobs: insert ignored duplicate id={}", job.id);
        return Ok(false);
    }
    log::info!(
        "cron_jobs: inserted id={} label={} kind={kind} expr={} next_run_at_ms={:?}",
        job.id,
        job.label,
        job.cron_expr,
        next
    );
    Ok(true)
}

/// List all enabled cron jobs whose `next_run_at_ms` is due (<= now or NULL).
/// The scheduler dispatches one TriggerRequest per due row.
pub fn list_due(conn: &Connection, now_ms: i64) -> Result<Vec<CronJobRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, label, cron_expr, schedule_kind, schedule_raw, conversation_id, current_session_id,
                prompt_text, agent_mode, lead_agent_id, enabled, last_run_at_ms,
                next_run_at_ms, created_at_ms, deliver, last_delivery_error
           FROM cron_jobs
          WHERE enabled = 1
            AND (next_run_at_ms IS NULL OR next_run_at_ms <= ?1)",
    )?;
    let rows = stmt
        .query_map(params![now_ms], row_to_record)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// List all cron jobs (enabled and disabled) for management endpoints.
pub fn list_all(conn: &Connection) -> Result<Vec<CronJobRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, label, cron_expr, schedule_kind, schedule_raw, conversation_id, current_session_id,
                prompt_text, agent_mode, lead_agent_id, enabled, last_run_at_ms,
                next_run_at_ms, created_at_ms, deliver, last_delivery_error
           FROM cron_jobs
          ORDER BY created_at_ms ASC",
    )?;
    let rows = stmt
        .query_map([], row_to_record)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<CronJobRecord>> {
    let row = conn
        .query_row(
            "SELECT id, label, cron_expr, schedule_kind, schedule_raw, conversation_id, current_session_id,
                    prompt_text, agent_mode, lead_agent_id, enabled, last_run_at_ms,
                    next_run_at_ms, created_at_ms, deliver, last_delivery_error
               FROM cron_jobs WHERE id = ?1",
            params![id],
            row_to_record,
        )
        .optional()?;
    Ok(row)
}

/// After a scheduled run fires, record the run time and advance or soft-complete.
///
/// - **once**: disable the job, clear `next_run_at_ms` (soft complete — row kept).
/// - **cron**: recompute next fire; if none, disable.
pub fn mark_ran<Z: TimeZone>(conn: &Connection, id: &str, ran_at: DateTime<Z>) -> Result<()> {
    let (kind, expr): (String, String) = conn.query_row(
        "SELECT COALESCE(schedule_kind, 'cron'), cron_expr FROM cron_jobs WHERE id = ?1",
        params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if kind == SCHEDULE_KIND_ONCE {
        conn.execute(
            "UPDATE cron_jobs SET last_run_at_ms = ?2, next_run_at_ms = NULL, enabled = 0 WHERE id = ?1",
            params![id, ran_at.timestamp_millis()],
        )?;
        log::info!(
            "cron_jobs: id={} one-shot soft-completed at={}",
            id,
            ran_at.to_rfc3339()
        );
        return Ok(());
    }
    // `ran_at` carries the timezone used to interpret the cron fields; pass it
    // through so the next firing is computed in the same (local) timezone.
    let next = next_run_ms(&expr, &ran_at);
    if next.is_none() {
        log::warn!("cron_jobs: id={} has no future firings; disabling", id);
        conn.execute(
            "UPDATE cron_jobs SET last_run_at_ms = ?2, next_run_at_ms = NULL, enabled = 0 WHERE id = ?1",
            params![id, ran_at.timestamp_millis()],
        )?;
        return Ok(());
    }
    conn.execute(
        "UPDATE cron_jobs SET last_run_at_ms = ?2, next_run_at_ms = ?3 WHERE id = ?1",
        params![id, ran_at.timestamp_millis(), next],
    )?;
    log::info!(
        "cron_jobs: id={} ran_at={} next_run_at_ms={:?}",
        id,
        ran_at.to_rfc3339(),
        next
    );
    Ok(())
}

/// Enable / disable a cron job. When enabling a recurring job, recompute
/// `next_run_at_ms` from now. One-shot jobs **cannot** be re-enabled (create a
/// new job instead).
pub fn set_enabled(conn: &Connection, id: &str, enabled: bool) -> Result<bool> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT COALESCE(schedule_kind, 'cron'), cron_expr FROM cron_jobs WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((kind, expr)) = row else {
        return Ok(false);
    };
    if enabled && kind == SCHEDULE_KIND_ONCE {
        anyhow::bail!("one-shot cron jobs cannot be re-enabled; create a new job");
    }
    let next = if enabled {
        next_run_ms(&expr, &Local::now())
    } else {
        None
    };
    conn.execute(
        "UPDATE cron_jobs SET enabled = ?2, next_run_at_ms = ?3 WHERE id = ?1",
        params![id, enabled as i32, next],
    )?;
    log::info!("cron_jobs: id={} enabled={}", id, enabled);
    Ok(true)
}

pub fn delete(conn: &Connection, id: &str) -> Result<bool> {
    let affected = conn.execute("DELETE FROM cron_jobs WHERE id = ?1", params![id])?;
    if affected > 0 {
        log::info!("cron_jobs: deleted id={}", id);
    }
    Ok(affected > 0)
}

fn row_to_record(r: &rusqlite::Row<'_>) -> rusqlite::Result<CronJobRecord> {
    Ok(CronJobRecord {
        id: r.get(0)?,
        label: r.get(1)?,
        cron_expr: r.get(2)?,
        schedule_kind: r
            .get::<_, Option<String>>(3)?
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| SCHEDULE_KIND_CRON.to_string()),
        schedule_raw: r.get(4)?,
        conversation_id: r.get(5)?,
        current_session_id: r.get(6)?,
        prompt_text: r.get(7)?,
        agent_mode: r.get(8)?,
        lead_agent_id: r.get(9)?,
        enabled: r.get::<_, i32>(10)? != 0,
        last_run_at_ms: r.get(11)?,
        next_run_at_ms: r.get(12)?,
        created_at_ms: r.get(13)?,
        deliver: r.get(14)?,
        last_delivery_error: r.get(15)?,
    })
}

/// JSON view of a cron job row (camelCase) shared by the server HTTP API and
/// the Tauri IPC commands so the frontend gets an identical shape on both
/// runtimes.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CronJobView {
    pub id: String,
    pub label: String,
    pub cron_expr: String,
    pub schedule_kind: String,
    pub schedule_raw: Option<String>,
    pub conversation_id: String,
    /// Active cron session id under which the current transcript lives. `None`
    /// until the scheduler first fires the job. The frontend's "查看会话" entry
    /// opens this id; prior ids (after a daily rollover) remain on disk but are
    /// not exposed here.
    pub current_session_id: Option<String>,
    pub prompt_text: String,
    pub agent_mode: Option<String>,
    pub lead_agent_id: Option<String>,
    pub enabled: bool,
    pub last_run_at_ms: Option<i64>,
    pub next_run_at_ms: Option<i64>,
    pub created_at_ms: i64,
    /// Optional Run → IM delivery spec. See [`CronJobRecord::deliver`].
    pub deliver: Option<String>,
    /// Last IM delivery error. Cleared when delivery succeeds.
    pub last_delivery_error: Option<String>,
}

impl CronJobView {
    /// Session id for the automation UI "查看会话" entry. Prefer the persisted
    /// active id; fall back to deriving from `last_run_at_ms` so jobs that ran
    /// before the `current_session_id` column migration remain viewable.
    pub fn resolve_view_session_id(r: &CronJobRecord) -> Option<String> {
        if let Some(ref id) = r.current_session_id {
            if !id.trim().is_empty() {
                return Some(id.clone());
            }
        }
        r.last_run_at_ms.map(|ms| {
            let dt = Local
                .timestamp_millis_opt(ms)
                .single()
                .unwrap_or_else(Local::now);
            current_cron_session_id(&r.id, &dt)
        })
    }

    pub fn from_record(r: &CronJobRecord) -> Self {
        Self {
            id: r.id.clone(),
            label: r.label.clone(),
            cron_expr: r.cron_expr.clone(),
            schedule_kind: r.schedule_kind.clone(),
            schedule_raw: r.schedule_raw.clone(),
            conversation_id: r.conversation_id.clone(),
            current_session_id: Self::resolve_view_session_id(r),
            prompt_text: r.prompt_text.clone(),
            agent_mode: r.agent_mode.clone(),
            lead_agent_id: r.lead_agent_id.clone(),
            enabled: r.enabled,
            last_run_at_ms: r.last_run_at_ms,
            next_run_at_ms: r.next_run_at_ms,
            created_at_ms: r.created_at_ms,
            deliver: r.deliver.clone(),
            last_delivery_error: r.last_delivery_error.clone(),
        }
    }
}

/// Update the job's deliver spec. Pass `None` / empty to clear IM auto-delivery.
pub fn update_deliver(conn: &Connection, id: &str, deliver: Option<&str>) -> Result<bool> {
    let deliver = deliver.map(str::trim).filter(|s| !s.is_empty());
    let affected = conn.execute(
        "UPDATE cron_jobs SET deliver = ?2 WHERE id = ?1",
        params![id, deliver],
    )?;
    if affected > 0 {
        log::info!(
            "cron_jobs: id={} deliver updated to {:?}",
            id,
            deliver
        );
    }
    Ok(affected > 0)
}

/// Persist the last IM delivery error (or clear it with `None` on success).
pub fn set_last_delivery_error(
    conn: &Connection,
    id: &str,
    err: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE cron_jobs SET last_delivery_error = ?2 WHERE id = ?1",
        params![id, err],
    )?;
    match err {
        Some(e) => log::warn!("cron_jobs: id={id} last_delivery_error={e}"),
        None => log::info!("cron_jobs: id={id} last_delivery_error cleared"),
    }
    Ok(())
}

/// Advance the job's active cron session id (on first fire or daily rollover).
/// The caller ensures the corresponding conversation meta row exists and loads
/// the transcript under this id. Prior ids' transcripts are left untouched.
pub fn set_current_session_id(conn: &Connection, id: &str, session_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE cron_jobs SET current_session_id = ?2 WHERE id = ?1",
        params![id, session_id],
    )?;
    Ok(())
}

/// Create the `cron_jobs` table (idempotent). Called from `init_schema`.
pub fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS cron_jobs (
           id TEXT PRIMARY KEY,
           label TEXT NOT NULL,
           cron_expr TEXT NOT NULL,
           schedule_kind TEXT NOT NULL DEFAULT 'cron',
           schedule_raw TEXT,
           conversation_id TEXT NOT NULL,
           current_session_id TEXT,
           prompt_text TEXT NOT NULL,
           agent_mode TEXT,
           lead_agent_id TEXT,
           enabled INTEGER NOT NULL DEFAULT 1,
           last_run_at_ms INTEGER,
           next_run_at_ms INTEGER,
           created_at_ms INTEGER NOT NULL,
           deliver TEXT,
           last_delivery_error TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_cron_jobs_by_conv
           ON cron_jobs(conversation_id);",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn insert_and_list_due() {
        let conn = mem();
        let job = NewCronJob {
            id: "j1",
            label: "every-minute",
            cron_expr: "0 * * * * *",
            schedule_kind: SCHEDULE_KIND_CRON,
            schedule_raw: Some("every_minute"),
            next_run_at_ms: None,
            conversation_id: "c1",
            prompt_text: "ping",
            agent_mode: None,
            lead_agent_id: None,
            enabled: true,
            deliver: None,
        };
        assert!(insert(&conn, &job).unwrap());
        // next_run is in the future relative to a far-future now, so list_due
        // with now=0 should return it (next_run_at_ms > 0 > ... actually <= 0?).
        // Use a now far in the future to make it due.
        let due = list_due(&conn, i64::MAX).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, "j1");
    }

    #[test]
    fn set_enabled_recomputes_next() {
        let conn = mem();
        insert(
            &conn,
            &NewCronJob {
                id: "j2",
                label: "off",
                cron_expr: "0 * * * * *",
                schedule_kind: SCHEDULE_KIND_CRON,
                schedule_raw: None,
                next_run_at_ms: None,
                conversation_id: "c2",
                prompt_text: "x",
                agent_mode: None,
                lead_agent_id: None,
                enabled: false,
                deliver: None,
            },
        )
        .unwrap();
        assert!(set_enabled(&conn, "j2", true).unwrap());
        let rec = get(&conn, "j2").unwrap().unwrap();
        assert!(rec.enabled);
        assert!(rec.next_run_at_ms.is_some());
    }

    #[test]
    fn mark_ran_advances_next() {
        let conn = mem();
        insert(
            &conn,
            &NewCronJob {
                id: "j3",
                label: "adv",
                cron_expr: "0 * * * * *",
                schedule_kind: SCHEDULE_KIND_CRON,
                schedule_raw: None,
                next_run_at_ms: None,
                conversation_id: "c3",
                prompt_text: "y",
                agent_mode: None,
                lead_agent_id: None,
                enabled: true,
                deliver: None,
            },
        )
        .unwrap();
        let before = get(&conn, "j3").unwrap().unwrap().next_run_at_ms;
        // Advance ran_at past the insert-time next fire so the recomputed
        // next is strictly later (insert next is <= 60s away).
        let ran_at = Utc::now() + chrono::Duration::seconds(65);
        mark_ran(&conn, "j3", ran_at).unwrap();
        let after = get(&conn, "j3").unwrap().unwrap().next_run_at_ms;
        assert!(after > before, "next_run must advance after mark_ran");
    }

    #[test]
    fn once_insert_due_soft_complete() {
        let conn = mem();
        let fire = Utc::now().timestamp_millis() - 1000;
        assert!(insert(
            &conn,
            &NewCronJob {
                id: "once1",
                label: "remind",
                cron_expr: "@once",
                schedule_kind: SCHEDULE_KIND_ONCE,
                schedule_raw: Some("30m"),
                next_run_at_ms: Some(fire),
                conversation_id: "",
                prompt_text: "ping me",
                agent_mode: None,
                lead_agent_id: None,
                enabled: true,
                deliver: None,
            },
        )
        .unwrap());
        let due = list_due(&conn, Utc::now().timestamp_millis()).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].schedule_kind, SCHEDULE_KIND_ONCE);
        mark_ran(&conn, "once1", Local::now()).unwrap();
        let rec = get(&conn, "once1").unwrap().unwrap();
        assert!(!rec.enabled);
        assert!(rec.next_run_at_ms.is_none());
        assert!(rec.last_run_at_ms.is_some());
        assert!(list_due(&conn, i64::MAX).unwrap().is_empty());
        let err = set_enabled(&conn, "once1", true).unwrap_err();
        assert!(
            err.to_string().contains("cannot be re-enabled"),
            "got {err}"
        );
    }

    #[test]
    fn daily_reset_boundary_is_today_then_yesterday() {
        // 10:00 local today -> reset boundary is today 04:00.
        let tz = chrono::Local;
        let today_10 = tz
            .with_ymd_and_hms(2026, 6, 28, 10, 0, 0)
            .single()
            .unwrap();
        let reset = daily_reset_at_ms(&today_10, 4);
        let expected = tz
            .with_ymd_and_hms(2026, 6, 28, 4, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis();
        assert_eq!(reset, expected);

        // 02:00 local (before today's 04:00) -> boundary is yesterday 04:00.
        let today_02 = tz
            .with_ymd_and_hms(2026, 6, 28, 2, 0, 0)
            .single()
            .unwrap();
        let reset_early = daily_reset_at_ms(&today_02, 4);
        let expected_yesterday = tz
            .with_ymd_and_hms(2026, 6, 27, 4, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis();
        assert_eq!(reset_early, expected_yesterday);
    }

    #[test]
    fn current_session_id_is_stable_within_reset_window() {
        let tz = chrono::Local;
        // 10:00 and 15:00 on the same day both fall after today's 04:00, so
        // they share the same reset-day label (today) → same session id.
        let morning = tz.with_ymd_and_hms(2026, 6, 28, 10, 0, 0).single().unwrap();
        let afternoon = tz.with_ymd_and_hms(2026, 6, 28, 15, 0, 0).single().unwrap();
        let a = current_cron_session_id("job1", &morning);
        let b = current_cron_session_id("job1", &afternoon);
        assert_eq!(a, b);
        assert!(a.starts_with("cron:job1:20260628"), "got {a}");

        // 02:00 (before today's 04:00) belongs to yesterday's reset window.
        let early = tz.with_ymd_and_hms(2026, 6, 28, 2, 0, 0).single().unwrap();
        let c = current_cron_session_id("job1", &early);
        assert!(c.starts_with("cron:job1:20260627"), "got {c}");

        // Crossing 04:00 yields a new id (rollover), old id differs.
        let after_reset = tz.with_ymd_and_hms(2026, 6, 28, 4, 30, 0).single().unwrap();
        let d = current_cron_session_id("job1", &after_reset);
        assert_ne!(c, d);
    }

    #[test]
    fn view_session_id_falls_back_to_last_run() {
        let r = CronJobRecord {
            id: "job1".into(),
            label: "test".into(),
            cron_expr: "0 * * * * *".into(),
            schedule_kind: SCHEDULE_KIND_CRON.into(),
            schedule_raw: None,
            conversation_id: "cron:job1".into(),
            current_session_id: None,
            prompt_text: "hi".into(),
            agent_mode: None,
            lead_agent_id: None,
            enabled: true,
            last_run_at_ms: Some(
                chrono::Local
                    .with_ymd_and_hms(2026, 6, 28, 10, 0, 0)
                    .single()
                    .unwrap()
                    .timestamp_millis(),
            ),
            next_run_at_ms: None,
            created_at_ms: 0,
            deliver: None,
            last_delivery_error: None,
        };
        let view = CronJobView::from_record(&r);
        assert_eq!(
            view.current_session_id.as_deref(),
            Some("cron:job1:20260628")
        );
    }
}
