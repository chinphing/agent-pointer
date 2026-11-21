//! Cron-based scheduler for the trigger dispatcher (Phase 5).
//!
//! A single ticker task wakes every `TICK_INTERVAL`, loads due `cron_jobs`
//! rows from the conversation store, and dispatches one [`TriggerRequest`]
//! per due job through the shared [`RunDispatcher`]. Each firing carries an
//! idempotency key derived from the job id + scheduled time, so a repeated
//! tick for the same scheduled slot is deduplicated by the dispatcher.
//!
//! Enablement is host-specific:
//! - **Web / server**: started by default (cron is a server-side capability).
//! - **Desktop / Tauri**: started by default (same as server). Disable via
//!   `POINTER_SCHEDULER_ENABLED=0`.
//!
//! Design borrows from openclaw `cron_loop` (DB-driven schedule + ticker) and
//! hermes (single dispatch entry reused by every trigger source).

use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use chrono::Utc;
use tokio_util::sync::CancellationToken;

use crate::chat_service::AppState;
use crate::conversation_store::cron_jobs::CronJobRecord;
use crate::conversation_store::ConversationStore;
use crate::dispatcher::{RunDispatcher, TriggerMeta, TriggerRequest, TriggerSource};
use crate::models::{ChatMessage, StreamEvent};
use crate::stream_broadcast;

/// Default ticker interval. The cron `Schedule` determines the actual firing
/// time; this is only the polling granularity. 60s keeps next-fire latency
/// under a minute while staying cheap (one index scan per tick).
const TICK_INTERVAL: Duration = Duration::from_secs(60);

/// Cron scheduler. Owns the ticker task handle so the host can stop it on
/// shutdown.
pub struct Scheduler {
    state: Arc<AppState>,
    dispatcher: Arc<RunDispatcher>,
    cancel: CancellationToken,
}

impl Scheduler {
    pub fn new(state: Arc<AppState>, dispatcher: Arc<RunDispatcher>) -> Self {
        Self {
            state,
            dispatcher,
            cancel: CancellationToken::new(),
        }
    }

    /// Spawn the ticker loop on the **current tokio runtime**. Returns the
    /// scheduler so the host can hold the handle and call [`Self::stop`] on
    /// shutdown. The task runs until the process exits or `stop` is called.
    ///
    /// **Only call this from within a tokio runtime context** (e.g. the
    /// server's `#[tokio::main]`). Tauri desktop's `setup` closure runs on the
    /// UI thread outside the tokio runtime, so desktop hosts must use
    /// [`Self::run`] with `tauri::async_runtime::spawn` instead — calling this
    /// from Tauri setup panics ("no reactor running") and aborts the process.
    pub fn start(self) -> Arc<Self> {
        let arc = Arc::new(self);
        let runner = arc.clone();
        tokio::spawn(async move {
            runner.run().await;
        });
        arc
    }

    /// The ticker loop itself. Hosts that cannot call [`Self::start`] (notably
    /// Tauri desktop, whose `setup` runs outside the tokio runtime) spawn this
    /// on their own async runtime, e.g.
    /// `tauri::async_runtime::spawn(scheduler.clone().run())`.
    pub async fn run(self: Arc<Self>) {
        log::info!("scheduler: ticker started (interval={:?})", TICK_INTERVAL);
        loop {
            self.tick().await;
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    log::info!("scheduler: ticker cancelled, exiting");
                    break;
                }
                _ = tokio::time::sleep(TICK_INTERVAL) => {}
            }
        }
    }

    /// Stop the ticker. In-flight dispatches are not cancelled (they run to
    /// completion under the dispatcher's own cancellation token).
    pub fn stop(&self) {
        self.cancel.cancel();
    }

    /// One polling pass: load due jobs, dispatch each, advance next_run.
    async fn tick(&self) {
        let now_ms = Utc::now().timestamp_millis();
        let due: Vec<CronJobRecord> = match self.state.session_index.cron_jobs_list_due(now_ms) {
            Ok(rows) => rows,
            Err(e) => {
                log::warn!("scheduler: list_due failed: {e:#}");
                return;
            }
        };
        if due.is_empty() {
            return;
        }
        log::info!("scheduler: {} due job(s) at now_ms={now_ms}", due.len());
        for job in due {
            self.dispatch_job(&job).await;
            // Advance next_run regardless of dispatch outcome so a failing
            // dispatch does not re-fire on the next tick for the same slot.
            // Pass `Local::now()` so the cron expression is interpreted in the
            // user's local timezone when recomputing the next firing.
            if let Err(e) = self
                .state
                .session_index
                .cron_jobs_mark_ran(&job.id, chrono::Local::now())
            {
                log::warn!("scheduler: mark_ran failed id={}: {e:#}", job.id);
            }
        }
    }

    async fn dispatch_job(&self, job: &CronJobRecord) {
        let scheduled_ms = job
            .next_run_at_ms
            .unwrap_or_else(|| Utc::now().timestamp_millis());
        // Idempotency key scopes to this scheduled slot; a repeated tick for
        // the same slot reuses the existing run instead of double-firing.
        let idempotency_key = format!("cron:{}:{}", job.id, scheduled_ms);
        // Each cron job owns a dedicated isolated session `cron:{job_id}`
        // (OpenClaw model). The session is reused across ticks so the agent
        // continues the same transcript — prior turns are loaded and prepended
        // to the new prompt, letting run_chat's history compression manage
        // length over time.
        // Each cron job owns a dedicated isolated session whose id encodes the
        // reset-day: `cron:{job_id}:{yyyymmdd}` (the calendar date of the most
        // recent 04:00 local boundary). All ticks within the same
        // [D 04:00, D+1 04:00) window share this id and continue one transcript;
        // crossing the boundary yields a new id → a fresh transcript, while the
        // prior id's transcript stays on disk (openclaw retention model). The
        // active id is persisted on the cron_jobs row so a process restart
        // resumes the same session within the day.
        let now_local = chrono::Local::now();
        let expected_session_id =
            crate::conversation_store::cron_jobs::current_cron_session_id(&job.id, &now_local);
        let needs_new_session = job.current_session_id.as_deref() != Some(&expected_session_id);
        if needs_new_session {
            if let Err(e) = self
                .state
                .session_index
                .cron_jobs_set_current_session_id(&job.id, &expected_session_id)
            {
                log::warn!(
                    "scheduler: set_current_session_id failed id={}: {e:#}",
                    job.id
                );
            }
            log::info!(
                "scheduler: cron session {} id={} session={}",
                if job.current_session_id.is_none() {
                    "first-fire"
                } else {
                    "rollover"
                },
                job.id,
                expected_session_id
            );
        }
        // Ownership: the cron session row inherits the job creator's
        // `session_user_id` (created here on first fire / rollover) so the run
        // resolves the creator's environment tier, runtime settings, memory and
        // workspace instead of the deployment default.
        if let Err(e) = ensure_cron_session_owner(
            &self.state.session_index,
            &expected_session_id,
            &job.label,
            job.owner_user_id.as_deref(),
            &job.id,
        ) {
            log::warn!(
                "scheduler: cron session owner stamp failed id={}: {e:#}",
                job.id
            );
        }
        // Hermes-aligned: cron execution guidance is prepended to the user
        // message (`build_cron_user_prompt`), not injected as a system block.
        // Broadcast so a live-open cron session UI sees the user turn (same
        // pattern as IM inbound); otherwise only assistant stream events appear.
        // Delta only — `prepare_lead_history` reloads the lead working set.
        // A first-fire / rollover session has no prior rows, so this is the
        // whole transcript until the turn writes more.
        let user_msg = ChatMessage::user_text(build_cron_user_prompt(&job.prompt_text));
        let user_msg_id = user_msg.id.clone();
        let user_msg_content = user_msg.content.clone();
        let messages = vec![user_msg];
        stream_broadcast::broadcast_stream(&StreamEvent::InjectedUserMessage {
            conversation_id: expected_session_id.clone(),
            message_id: user_msg_id,
            content: user_msg_content,
            attachments: None,
            ui_bindings: None,
        });
        log::info!(
            "scheduler: dispatching job id={} session={} dispatch_messages={} prompt_len={} deliver={:?}",
            job.id,
            expected_session_id,
            messages.len(),
            job.prompt_text.len(),
            job.deliver,
        );
        // Resolve the deliver target marker. The structured `DeliverTarget::Im`
        // variant is used only as an in-flight marker (channel = first segment
        // of the deliver spec); the authoritative spec lives in
        // `trigger_meta.extra.deliver`, which is persisted to `runs.trigger_meta_json`
        // and read back by `ImDeliverHook` on run finish (survives restarts).
        let deliver_marker = crate::dispatcher::resolve_deliver_marker(job.deliver.as_deref());
        let trigger_meta = build_trigger_meta(&job.id, job.deliver.as_deref());
        let req = TriggerRequest {
            run_id: None,
            idempotency_key: Some(idempotency_key),
            conversation_id: Some(expected_session_id),
            trigger_source: TriggerSource::Cron,
            trigger_meta,
            lane: None,
            messages,
            enabled_skill_ids: Vec::new(),
            // Empty → run_chat loads agentSkillOverrides from user_settings.
            agent_skill_overrides: std::collections::HashMap::new(),
            lead_agent_id: job.lead_agent_id.clone(),
            performance_mode: None,
            tool_rounds_used_single_start: 0,
            workspace_root: String::new(),
            workspace_inherit_disabled: None,
            deliver: deliver_marker,
            web_session_auth: self.state.automation_execution_auth(),
            access_host: None,
        };
        match self.dispatcher.dispatch(req).await {
            Ok(handle) => log::info!(
                "scheduler: dispatched job id={} run_id={} status={:?}",
                job.id,
                handle.run_id,
                handle.status
            ),
            Err(e) => log::error!("scheduler: dispatch failed job id={}: {e:#}", job.id),
        }
    }
}

/// Ensure the cron session row exists and carries the job creator's identity.
///
/// The row is created on first fire / daily rollover (`cron:{job_id}:{yyyymmdd}`)
/// and only gains an owner when it has none — an existing binding always wins
/// (`ensure_session_user_id` semantics). Once the row carries the creator's
/// `session_user_id`, the cron turn resolves that user's environment tier,
/// runtime settings, memory and workspace instead of the deployment default.
///
/// A job without an owner (legacy row written before `owner_user_id` existed)
/// leaves the session anonymous → deployment default tier, and warns once per
/// job rather than on every tick.
pub fn ensure_cron_session_owner(
    store: &ConversationStore,
    session_id: &str,
    title: &str,
    owner_user_id: Option<&str>,
    job_id: &str,
) -> anyhow::Result<()> {
    store.ensure_cron_session(session_id, title)?;
    let existing = store.session_user_id(session_id)?;
    if !existing.trim().is_empty() {
        return Ok(());
    }
    match owner_user_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(owner) => {
            store.ensure_session_user_id(session_id, owner)?;
            log::info!(
                "scheduler: cron session {session_id} owner={owner} id={job_id}"
            );
        }
        None => warn_missing_cron_owner_once(job_id, session_id),
    }
    Ok(())
}

/// Warn once per job about a missing owner record (legacy rows), so a due job
/// does not spam the log every tick.
fn warn_missing_cron_owner_once(job_id: &str, session_id: &str) {
    static WARNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let warned = WARNED.get_or_init(|| Mutex::new(HashSet::new()));
    let mut guard = match warned.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if guard.insert(job_id.to_string()) {
        log::warn!(
            "scheduler: cron job id={job_id} 没有归属记录（owner_user_id 为空），会话 {session_id} 按部署默认档位执行"
        );
    }
}

/// Hermes-aligned cron tick user message: prepend execution guidance to the
/// job's `prompt_text`, then pass the whole string as the user turn
/// (`run_conversation(user_message=…)`). Not a system prompt block.
pub fn build_cron_user_prompt(prompt_text: &str) -> String {
    format!("{CRON_USER_HINT}{}", prompt_text.trim_start())
}

/// System prompt for non-cron runs that requested IM auto-delivery
/// (HTTP Runs / Webhook). Cron uses [`build_cron_user_prompt`] instead.
pub fn auto_deliver_system_prompt() -> String {
    AUTO_DELIVER_HINT.to_string()
}

/// Mirrors hermes `cron/scheduler.py` `_build_job_prompt` `cron_hint`
/// (`im_send` instead of `send_message`). Always prepended on every tick.
pub(crate) const CRON_USER_HINT: &str = "\
[IMPORTANT: You are running as a scheduled cron job. \
DELIVERY: Your final response will be automatically delivered \
to the user — do NOT use im_send or try to deliver \
the output yourself. Just produce your report/output as your \
final response and the system handles the rest. \
SILENT: If there is genuinely nothing new to report, respond \
with exactly \"[SILENT]\" (nothing else) to suppress delivery. \
Never combine [SILENT] with content — either report your \
findings normally, or say [SILENT] and nothing more.]\n\n";

const AUTO_DELIVER_HINT: &str =
    "Your final reply is auto-delivered to IM — do not call im_send to duplicate it.\n\
If there is nothing to report, reply with exactly [SILENT] and nothing else.";

/// Build the `TriggerMeta` for a cron tick. `job_id` is always set so the
/// delivery hook can correlate the run back to its cron job; `extra.deliver`
/// carries the authoritative delivery spec (persisted to `runs.trigger_meta_json`).
fn build_trigger_meta(job_id: &str, deliver: Option<&str>) -> TriggerMeta {
    let mut meta = TriggerMeta {
        job_id: Some(job_id.to_string()),
        ..TriggerMeta::empty()
    };
    let _ = crate::dispatcher::apply_deliver_string(&mut meta, deliver);
    meta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_run_ms_parses_valid_expr() {
        // Every minute.
        let next = next_run_ms("0 * * * * *", Utc::now());
        assert!(next.is_some());
    }

    #[test]
    fn next_run_ms_rejects_invalid_expr() {
        assert!(next_run_ms("not a cron expr", Utc::now()).is_none());
    }

    #[test]
    fn build_cron_user_prompt_prepends_hermes_hint() {
        let p = build_cron_user_prompt("Remind the user to drink water");
        assert!(p.starts_with("[IMPORTANT:"));
        assert!(p.contains("scheduled cron job"));
        assert!(p.contains("im_send"));
        assert!(p.contains("[SILENT]"));
        assert!(p.contains("Remind the user to drink water"));
    }

    // Re-export the free function for the test helpers above.
    fn next_run_ms(expr: &str, after: chrono::DateTime<chrono::Utc>) -> Option<i64> {
        crate::conversation_store::cron_jobs::next_run_ms(expr, &after)
    }

    // ---- cron session ownership (owner_user_id → conversations.session_user_id) ----

    use crate::conversation_store::cron_jobs::{self, NewCronJob, SCHEDULE_KIND_CRON};
    use crate::env::provider::{AdminTier, ProviderConfig, Tier};
    use chrono::TimeZone;

    fn test_store() -> (tempfile::TempDir, ConversationStore) {
        let dir = tempfile::TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        (dir, store)
    }

    fn insert_job(store: &ConversationStore, id: &str, owner: Option<&str>) {
        assert!(store
            .cron_jobs_insert(&NewCronJob {
                id,
                label: "nightly",
                cron_expr: "0 0 9 * * *",
                schedule_kind: SCHEDULE_KIND_CRON,
                schedule_raw: Some("daily@9:00"),
                next_run_at_ms: None,
                conversation_id: "",
                prompt_text: "report",
                lead_agent_id: None,
                enabled: true,
                deliver: None,
                owner_user_id: owner,
            })
            .unwrap());
    }

    /// Deployment default `headless` + admin list containing `admin`, mirroring
    /// a self-hosted deployment where admins must run on the host (`local`).
    fn provider() -> ProviderConfig {
        ProviderConfig {
            tier: Some(Tier::Headless),
            admin_tier: AdminTier::Local,
            admins: vec!["admin".to_string()],
            sandbox_url: Some("http://127.0.0.1:3000".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn cron_session_inherits_admin_owner_and_resolves_local_tier() {
        let (_dir, store) = test_store();
        insert_job(&store, "job-admin", Some("admin"));
        let rec = store.cron_jobs_get("job-admin").unwrap().unwrap();
        let session = cron_jobs::current_cron_session_id(&rec.id, &chrono::Local::now());
        ensure_cron_session_owner(
            &store,
            &session,
            &rec.label,
            rec.owner_user_id.as_deref(),
            &rec.id,
        )
        .unwrap();
        assert_eq!(store.session_user_id(&session).unwrap(), "admin");
        // The session row uid is what tier resolution reads → admin runs local.
        assert_eq!(provider().resolve(Some("admin")).unwrap().tier, Tier::Local);
    }

    #[test]
    fn cron_session_inherits_normal_user_and_keeps_default_tier() {
        let (_dir, store) = test_store();
        insert_job(&store, "job-bob", Some("bob"));
        let rec = store.cron_jobs_get("job-bob").unwrap().unwrap();
        let session = cron_jobs::current_cron_session_id(&rec.id, &chrono::Local::now());
        ensure_cron_session_owner(
            &store,
            &session,
            &rec.label,
            rec.owner_user_id.as_deref(),
            &rec.id,
        )
        .unwrap();
        assert_eq!(store.session_user_id(&session).unwrap(), "bob");
        assert_eq!(
            provider().resolve(Some("bob")).unwrap().tier,
            Tier::Headless,
            "non-admin creators keep the deployment default tier"
        );
    }

    #[test]
    fn daily_rollover_session_also_inherits_owner() {
        let (_dir, store) = test_store();
        insert_job(&store, "job-roll", Some("admin"));
        let rec = store.cron_jobs_get("job-roll").unwrap().unwrap();
        let tz = chrono::Local;
        let day1 = tz.with_ymd_and_hms(2026, 6, 28, 10, 0, 0).single().unwrap();
        let day2 = tz.with_ymd_and_hms(2026, 6, 29, 10, 0, 0).single().unwrap();
        let s1 = cron_jobs::current_cron_session_id(&rec.id, &day1);
        let s2 = cron_jobs::current_cron_session_id(&rec.id, &day2);
        assert_ne!(s1, s2, "crossing 04:00 must yield a new session id");
        for session in [&s1, &s2] {
            ensure_cron_session_owner(
                &store,
                session,
                &rec.label,
                rec.owner_user_id.as_deref(),
                &rec.id,
            )
            .unwrap();
            assert_eq!(store.session_user_id(session).unwrap(), "admin");
        }
    }

    #[test]
    fn legacy_job_without_owner_keeps_session_anonymous() {
        let (_dir, store) = test_store();
        insert_job(&store, "job-legacy", None);
        let rec = store.cron_jobs_get("job-legacy").unwrap().unwrap();
        assert_eq!(rec.owner_user_id, None);
        let session = cron_jobs::current_cron_session_id(&rec.id, &chrono::Local::now());
        // Must not error, must not invent an owner.
        ensure_cron_session_owner(
            &store,
            &session,
            &rec.label,
            rec.owner_user_id.as_deref(),
            &rec.id,
        )
        .unwrap();
        assert_eq!(store.session_user_id(&session).unwrap(), "");
        assert_eq!(
            provider().resolve(Some("")).unwrap().tier,
            Tier::Headless,
            "unknown owner falls back to the deployment default tier"
        );
    }

    #[test]
    fn existing_session_owner_is_not_overwritten() {
        let (_dir, store) = test_store();
        let session = "cron:job-x:20260628";
        store.ensure_cron_session(session, "nightly").unwrap();
        store.ensure_session_user_id(session, "first").unwrap();
        ensure_cron_session_owner(&store, session, "nightly", Some("second"), "job-x").unwrap();
        assert_eq!(store.session_user_id(session).unwrap(), "first");
    }
}
