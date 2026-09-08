//! Defer `finalize_run` until background jobs for the same `run_id` finish.
//!
//! Background sub-agents reuse the parent `run_id`. If the lead `run_chat` ends
//! first and finalizes immediately, later `record_round` updates match 0 rows
//! (`report_status` is no longer `accumulating`) and tokens are dropped.
//!
//! Protocol:
//! 1. Parent end always parks the run in [`DeferredTokenFinalizeStore`].
//! 2. If no non-terminal jobs remain for that `run_id`, finalize immediately
//!    (with history archive).
//! 3. Otherwise wait; each background `finish` tries again. Deferred finalize
//!    uses an empty history (no archive) — same as stale recovery.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::models::ChatMessage;
use crate::platform_auth::PlatformAuthManager;

use super::app_state::AppState;
use super::job_supervisor::{JobStatus, JobSupervisor};

/// Parks `run_id → conversation_id` while the lead turn has ended but
/// background workers for that run may still accumulate tokens.
#[derive(Default)]
pub struct DeferredTokenFinalizeStore {
    pending: Mutex<HashMap<String, String>>,
}

impl DeferredTokenFinalizeStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn park(&self, run_id: &str, conversation_id: &str) {
        if run_id.trim().is_empty() {
            return;
        }
        self.pending
            .lock()
            .insert(run_id.to_string(), conversation_id.to_string());
    }

    /// Remove a parked run when idle (`running_count_for_run == 0`).
    fn take_if_idle(&self, jobs: &JobSupervisor, run_id: &str) -> Option<String> {
        if run_id.trim().is_empty() {
            return None;
        }
        let mut pending = self.pending.lock();
        if !pending.contains_key(run_id) {
            return None;
        }
        if jobs.running_count_for_run(run_id) > 0 {
            return None;
        }
        pending.remove(run_id)
    }

    #[cfg(test)]
    fn is_parked(&self, run_id: &str) -> bool {
        self.pending.lock().contains_key(run_id)
    }
}

async fn finalize_and_flush(
    run_id: &str,
    conversation_id: &str,
    history: &[ChatMessage],
    auth: &PlatformAuthManager,
) {
    if let Err(e) = crate::token_usage_store::finalize_run(run_id, conversation_id, history) {
        log::warn!(
            "token_usage_store: finalize_run failed run_id={run_id} conversation_id={conversation_id}: {e}"
        );
    }
    if let Err(e) = crate::token_usage_store::flush_unsent_reports(auth).await {
        log::warn!("token_usage_store: flush after finalize failed run_id={run_id}: {e}");
    }
}

/// Lead `run_chat` end: finalize now, or park until background jobs for `run_id` clear.
pub async fn on_parent_run_finished(
    state: &AppState,
    run_id: &str,
    conversation_id: &str,
    history: &[ChatMessage],
) {
    if run_id.trim().is_empty() {
        log::warn!(
            "token_usage_store: skip finalize; empty run_id conversation_id={conversation_id}"
        );
        return;
    }

    // Park first so a racing last-job finish cannot miss the parent-done signal.
    state
        .deferred_token_finalize
        .park(run_id, conversation_id);

    let running = state.jobs.running_count_for_run(run_id);
    if running > 0 {
        log::info!(
            "token_usage_store: defer finalize_run run_id={run_id} conversation_id={conversation_id} background_running={running}"
        );
        return;
    }

    let Some(cid) = state
        .deferred_token_finalize
        .take_if_idle(&state.jobs, run_id)
    else {
        // A background finish already took the park and finalized.
        log::info!(
            "token_usage_store: finalize already claimed by background finish run_id={run_id} conversation_id={conversation_id}"
        );
        return;
    };

    log::info!(
        "token_usage_store: finalize_run immediate run_id={run_id} conversation_id={cid}"
    );
    finalize_and_flush(
        run_id,
        &cid,
        history,
        state.active_platform_auth().as_ref(),
    )
    .await;
}

/// After a background job reaches a terminal status, finalize if parent already ended
/// and no other jobs remain for this `run_id`.
pub async fn on_background_job_finished(state: &AppState, run_id: &str) {
    let Some(conversation_id) = state
        .deferred_token_finalize
        .take_if_idle(&state.jobs, run_id)
    else {
        return;
    };
    log::info!(
        "token_usage_store: finalize_run after background idle run_id={run_id} conversation_id={conversation_id}"
    );
    // Deferred path: no lead history snapshot (billing does not require archive).
    finalize_and_flush(
        run_id,
        &conversation_id,
        &[],
        state.active_platform_auth().as_ref(),
    )
    .await;
}

/// `JobSupervisor::finish` then maybe finalize deferred token usage for that run.
pub async fn finish_job_and_maybe_finalize(
    state: &AppState,
    job_id: &str,
    status: JobStatus,
    content: Option<String>,
    error: Option<String>,
) {
    let Some(run_id) = state.jobs.finish(job_id, status, content, error) else {
        return;
    };
    on_background_job_finished(state, &run_id).await;
}

/// Convenience when callers already hold `Arc<AppState>`.
pub async fn finish_job_and_maybe_finalize_arc(
    state: &Arc<AppState>,
    job_id: &str,
    status: JobStatus,
    content: Option<String>,
    error: Option<String>,
) {
    finish_job_and_maybe_finalize(state.as_ref(), job_id, status, content, error).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_service::job_supervisor::{JobKind, JobKindSubagent};
    use tokio_util::sync::CancellationToken;

    fn subagent_kind() -> JobKind {
        JobKind::Subagent(JobKindSubagent {
            tool_call_id: "tc".into(),
            message_id: "m".into(),
            agent_id: "explore".into(),
            title: "t".into(),
            agent_instance_id: "inst".into(),
        })
    }

    #[tokio::test]
    async fn parent_defers_while_background_running_then_finalizes_on_last_finish() {
        let state = AppState::new();
        let run_id = "run-defer-1";
        let conv = "conv-defer-1";
        let job_id = state.jobs.register(
            conv,
            subagent_kind(),
            CancellationToken::new(),
            run_id,
        );
        state.jobs.mark_running(&job_id);

        on_parent_run_finished(&state, run_id, conv, &[]).await;
        assert!(state.deferred_token_finalize.is_parked(run_id));
        assert_eq!(state.jobs.running_count_for_run(run_id), 1);

        finish_job_and_maybe_finalize(
            &state,
            &job_id,
            JobStatus::Completed,
            Some("ok".into()),
            None,
        )
        .await;
        assert!(!state.deferred_token_finalize.is_parked(run_id));
        assert_eq!(state.jobs.running_count_for_run(run_id), 0);
    }

    #[tokio::test]
    async fn parent_finalizes_immediately_when_no_background_jobs() {
        let state = AppState::new();
        let run_id = "run-immediate-1";
        let conv = "conv-immediate-1";
        on_parent_run_finished(&state, run_id, conv, &[]).await;
        assert!(!state.deferred_token_finalize.is_parked(run_id));
    }

    #[tokio::test]
    async fn cancelling_one_of_two_keeps_park_until_last() {
        let state = AppState::new();
        let run_id = "run-partial-cancel";
        let conv = "conv-partial-cancel";
        let a = state.jobs.register(
            conv,
            subagent_kind(),
            CancellationToken::new(),
            run_id,
        );
        let b = state.jobs.register(
            conv,
            subagent_kind(),
            CancellationToken::new(),
            run_id,
        );
        state.jobs.mark_running(&a);
        state.jobs.mark_running(&b);

        on_parent_run_finished(&state, run_id, conv, &[]).await;
        assert!(state.deferred_token_finalize.is_parked(run_id));

        finish_job_and_maybe_finalize(
            &state,
            &a,
            JobStatus::Cancelled,
            None,
            Some("cancelled".into()),
        )
        .await;
        assert!(state.deferred_token_finalize.is_parked(run_id));

        finish_job_and_maybe_finalize(
            &state,
            &b,
            JobStatus::Completed,
            Some("ok".into()),
            None,
        )
        .await;
        assert!(!state.deferred_token_finalize.is_parked(run_id));
    }

    #[test]
    fn running_count_for_run_ignores_other_runs() {
        let state = AppState::new();
        let a = state.jobs.register(
            "c1",
            subagent_kind(),
            CancellationToken::new(),
            "run-a",
        );
        let _b = state.jobs.register(
            "c1",
            subagent_kind(),
            CancellationToken::new(),
            "run-b",
        );
        state.jobs.mark_running(&a);
        assert_eq!(state.jobs.running_count_for_run("run-a"), 1);
        assert_eq!(state.jobs.running_count_for_run("run-b"), 1);
        state
            .jobs
            .finish(&a, JobStatus::Completed, Some("x".into()), None);
        assert_eq!(state.jobs.running_count_for_run("run-a"), 0);
        assert_eq!(state.jobs.running_count_for_run("run-b"), 1);
    }
}
