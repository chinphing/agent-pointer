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

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio_util::sync::CancellationToken;

use crate::chat_service::AppState;
use crate::conversation_store::cron_jobs::CronJobRecord;
use crate::dispatcher::{
    RunDispatcher, TriggerMeta, TriggerRequest, TriggerSource,
};
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
        let scheduled_ms = job.next_run_at_ms.unwrap_or_else(|| Utc::now().timestamp_millis());
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
            if let Err(e) = self
                .state
                .session_index
                .ensure_cron_session(&expected_session_id, &job.label)
            {
                log::warn!(
                    "scheduler: ensure_cron_session failed id={}: {e:#}",
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
        // Load the active session's transcript; on a rollover/first-fire this is
        // empty (new session id), so the new prompt starts a fresh transcript.
        let mut messages = match self
            .state
            .session_index
            .load_messages(&expected_session_id)
        {
            Ok(history) => history,
            Err(e) => {
                log::warn!(
                    "scheduler: load cron session history failed id={} session={}: {e:#}",
                    job.id,
                    expected_session_id
                );
                Vec::new()
            }
        };
        // Construct the cron tick's user prompt. We keep a handle to the
        // message so we can broadcast it (see below) — the dispatcher/run_chat
        // persists it to the cron session via the transcript session's
        // `append_missing`, but a UI that has the cron conversation open live
        // only learns about new rows through stream events. Without this
        // broadcast the assistant reply streams in (message_start/delta/...)
        // while the preceding user prompt never appears in the open view,
        // producing an "extra assistant reply with no matching user message"
        // mismatch. Mirrors how IM channels surface inbound user messages.
        //
        // Delivery / [SILENT] guidance lives in the cron system prompt
        // (`cron_system_prompt`), not in the user message.
        let user_msg = ChatMessage::user_text(job.prompt_text.clone());
        let user_msg_id = user_msg.id.clone();
        let user_msg_content = user_msg.content.clone();
        messages.push(user_msg);
        stream_broadcast::broadcast_stream(&StreamEvent::InjectedUserMessage {
            conversation_id: expected_session_id.clone(),
            message_id: user_msg_id,
            content: user_msg_content,
            attachments: None,
        });
        log::info!(
            "scheduler: dispatching job id={} session={} history_len={} prompt_len={} deliver={:?}",
            job.id,
            expected_session_id,
            messages.len().saturating_sub(1),
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
            enabled_skill_ids: self.state.default_run_enabled_skill_ids(),
            agent_skill_overrides: std::collections::HashMap::new(),
            agent_mode: job.agent_mode.clone(),
            lead_agent_id: job.lead_agent_id.clone(),
            tool_rounds_used_single_start: 0,
            tool_rounds_used_supervisor_start: 0,
            workspace_root: String::new(),
            workspace_inherit_disabled: None,
            deliver: deliver_marker,
            web_session_auth: self.state.automation_execution_auth(),
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

/// Cron-only system prompt block (injected by `run_chat_inner` when
/// `TriggerSource::Cron`). Short hermes-style delivery hint when the job has
/// an IM `deliver` target; otherwise just identifies the run as scheduled.
pub fn cron_system_prompt(auto_deliver: bool) -> String {
    if auto_deliver {
        format!(
            "You are running as a scheduled cron job.\n{}",
            AUTO_DELIVER_HINT
        )
    } else {
        "You are running as a scheduled cron job.".to_string()
    }
}

/// System prompt for non-cron runs that requested IM auto-delivery
/// (HTTP Runs / Webhook).
pub fn auto_deliver_system_prompt() -> String {
    AUTO_DELIVER_HINT.to_string()
}

const AUTO_DELIVER_HINT: &str = "Your final reply is auto-delivered to IM — do not call im_send to duplicate it.\n\
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
    use crate::conversation_store::cron_jobs::NewCronJob;

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
    fn cron_system_prompt_short_without_deliver() {
        let p = cron_system_prompt(false);
        assert_eq!(p, "You are running as a scheduled cron job.");
        assert!(!p.contains("im_send"));
        assert!(!p.contains("[SILENT]"));
    }

    #[test]
    fn cron_system_prompt_includes_delivery_when_auto_deliver() {
        let p = cron_system_prompt(true);
        assert!(p.contains("scheduled cron job"));
        assert!(p.contains("im_send"));
        assert!(p.contains("[SILENT]"));
        assert!(!p.contains("[IMPORTANT:"));
    }

    // Re-export the free function for the test helpers above.
    fn next_run_ms(expr: &str, after: chrono::DateTime<chrono::Utc>) -> Option<i64> {
        crate::conversation_store::cron_jobs::next_run_ms(expr, &after)
    }
}
