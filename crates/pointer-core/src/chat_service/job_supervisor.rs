//! Background job table for `run_subagent.background` (and later terminal detach).
//!
//! Host-owned: concurrency cap, slot release on terminal, cancel fan-out.
//! Does not hold `session:{conversation}`. Parent `done` does not cancel jobs.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

const DEFAULT_AWAIT_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const CONTENT_PREVIEW_CHARS: usize = 800;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwaitMode {
    Any,
    All,
}

impl AwaitMode {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim).unwrap_or("any") {
            "all" => Self::All,
            _ => Self::Any,
        }
    }
}

#[derive(Debug, Clone)]
pub struct JobKindSubagent {
    pub tool_call_id: String,
    pub message_id: String,
    pub agent_id: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub enum JobKind {
    Subagent(JobKindSubagent),
}

#[derive(Debug, Clone)]
struct JobRecord {
    id: String,
    conversation_id: String,
    kind: JobKind,
    status: JobStatus,
    content: Option<String>,
    error: Option<String>,
    claimed: bool,
    cancel: CancellationToken,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobListItem {
    pub job_id: String,
    pub status: &'static str,
    pub kind: &'static str,
    pub agent_id: Option<String>,
    pub title: Option<String>,
    pub claimed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobAwaitItem {
    pub job_id: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobAwaitResult {
    pub mode: &'static str,
    pub timed_out: bool,
    pub jobs: Vec<JobAwaitItem>,
    pub running: Vec<String>,
    pub running_count: usize,
    pub slot_cap: usize,
}

#[derive(Default)]
struct Inner {
    jobs: HashMap<String, JobRecord>,
    by_conversation: HashMap<String, Vec<String>>,
}

pub struct JobSupervisor {
    inner: Mutex<Inner>,
    bump: watch::Sender<u64>,
    running_slots: AtomicUsize,
}

impl Default for JobSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl JobSupervisor {
    pub fn new() -> Self {
        let (bump, _) = watch::channel(0u64);
        Self {
            inner: Mutex::new(Inner::default()),
            bump,
            running_slots: AtomicUsize::new(0),
        }
    }

    fn notify(&self) {
        self.bump.send_modify(|v| *v = v.wrapping_add(1));
    }

    fn subscribe(&self) -> watch::Receiver<u64> {
        self.bump.subscribe()
    }

    pub fn slot_cap_from(max_parallel_sub_agents: usize) -> usize {
        max_parallel_sub_agents.max(1)
    }

    pub fn running_slot_count(&self) -> usize {
        self.running_slots.load(Ordering::SeqCst)
    }

    pub fn running_count_for_conversation(&self, conversation_id: &str) -> usize {
        let inner = self.inner.lock();
        inner
            .by_conversation
            .get(conversation_id)
            .map(|ids| {
                ids.iter()
                    .filter(|id| {
                        inner
                            .jobs
                            .get(*id)
                            .is_some_and(|j| !j.status.is_terminal())
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    /// Register a job in `queued`. Caller must spawn work that acquires a slot.
    pub fn register(
        &self,
        conversation_id: &str,
        kind: JobKind,
        cancel: CancellationToken,
    ) -> String {
        let id = format!("job_{}", uuid::Uuid::new_v4().simple());
        let record = JobRecord {
            id: id.clone(),
            conversation_id: conversation_id.to_string(),
            kind,
            status: JobStatus::Queued,
            content: None,
            error: None,
            claimed: false,
            cancel,
        };
        {
            let mut inner = self.inner.lock();
            inner
                .by_conversation
                .entry(conversation_id.to_string())
                .or_default()
                .push(id.clone());
            inner.jobs.insert(id.clone(), record);
        }
        log::info!(
            "job_supervisor: registered job_id={id} conversation_id={conversation_id} status=queued"
        );
        self.notify();
        id
    }

    pub fn cancel_token(&self, job_id: &str) -> Option<CancellationToken> {
        self.inner
            .lock()
            .jobs
            .get(job_id)
            .map(|j| j.cancel.clone())
    }

    pub fn mark_running(&self, job_id: &str) {
        let mut inner = self.inner.lock();
        let Some(job) = inner.jobs.get_mut(job_id) else {
            log::warn!("job_supervisor: mark_running unknown job_id={job_id}");
            return;
        };
        if job.status.is_terminal() {
            return;
        }
        job.status = JobStatus::Running;
        log::info!(
            "job_supervisor: job_id={job_id} conversation_id={} status=running",
            job.conversation_id
        );
        drop(inner);
        self.notify();
    }

    /// Wait for a running slot. Returns false if cancelled before acquiring.
    pub async fn acquire_slot(&self, cap: usize, cancel: &CancellationToken) -> bool {
        let cap = cap.max(1);
        let mut rx = self.subscribe();
        loop {
            if cancel.is_cancelled() {
                log::info!("job_supervisor: acquire_slot cancelled before permit cap={cap}");
                return false;
            }
            let current = self.running_slots.load(Ordering::SeqCst);
            if current < cap {
                match self.running_slots.compare_exchange(
                    current,
                    current + 1,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    Ok(_) => {
                        log::info!(
                            "job_supervisor: slot acquired running={} cap={}",
                            current + 1,
                            cap
                        );
                        return true;
                    }
                    Err(_) => continue,
                }
            }
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    log::info!("job_supervisor: acquire_slot cancelled while waiting cap={cap}");
                    return false;
                }
                changed = rx.changed() => {
                    if changed.is_err() {
                        log::warn!("job_supervisor: acquire_slot watch closed cap={cap}");
                        return false;
                    }
                }
            }
        }
    }

    pub fn release_slot(&self) {
        let prev = self.running_slots.fetch_sub(1, Ordering::SeqCst);
        if prev == 0 {
            self.running_slots.store(0, Ordering::SeqCst);
            log::warn!("job_supervisor: release_slot underflow, clamped to 0");
        } else {
            log::info!(
                "job_supervisor: slot released running={}",
                prev.saturating_sub(1)
            );
        }
        self.notify();
    }

    pub fn finish(
        &self,
        job_id: &str,
        status: JobStatus,
        content: Option<String>,
        error: Option<String>,
    ) {
        if !status.is_terminal() {
            log::warn!(
                "job_supervisor: finish ignored non-terminal status={:?} job_id={job_id}",
                status
            );
            return;
        }
        let mut inner = self.inner.lock();
        let Some(job) = inner.jobs.get_mut(job_id) else {
            log::warn!("job_supervisor: finish unknown job_id={job_id}");
            return;
        };
        if job.status.is_terminal() {
            log::info!("job_supervisor: finish idempotent job_id={job_id} status={:?}", job.status);
            return;
        }
        job.status = status;
        job.content = content;
        job.error = error;
        log::info!(
            "job_supervisor: job_id={job_id} conversation_id={} status={}",
            job.conversation_id,
            status.as_str()
        );
        drop(inner);
        self.notify();
    }

    pub fn list(&self, conversation_id: &str, include_content: bool) -> Vec<JobListItem> {
        let inner = self.inner.lock();
        let Some(ids) = inner.by_conversation.get(conversation_id) else {
            return Vec::new();
        };
        ids.iter()
            .filter_map(|id| inner.jobs.get(id).map(|j| job_list_item(j, include_content)))
            .collect()
    }

    pub fn status(&self, conversation_id: &str, job_id: &str) -> Result<JobListItem, String> {
        let inner = self.inner.lock();
        let Some(job) = inner.jobs.get(job_id) else {
            return Err(format!("unknown jobId `{job_id}`"));
        };
        if job.conversation_id != conversation_id {
            return Err("jobId does not belong to this conversation".into());
        }
        Ok(job_list_item(job, true))
    }

    pub fn cancel_ids(&self, conversation_id: &str, job_ids: Option<&[String]>) -> Vec<String> {
        let tokens: Vec<(String, CancellationToken)> = {
            let inner = self.inner.lock();
            let ids = match job_ids {
                Some(ids) if !ids.is_empty() => ids.to_vec(),
                _ => inner
                    .by_conversation
                    .get(conversation_id)
                    .cloned()
                    .unwrap_or_default(),
            };
            ids.into_iter()
                .filter_map(|id| {
                    let job = inner.jobs.get(&id)?;
                    if job.conversation_id != conversation_id {
                        return None;
                    }
                    if job.status.is_terminal() {
                        return None;
                    }
                    Some((id, job.cancel.clone()))
                })
                .collect()
        };
        let cancelled: Vec<String> = tokens.iter().map(|(id, _)| id.clone()).collect();
        for (id, token) in &tokens {
            log::info!(
                "job_supervisor: cancelling job_id={id} conversation_id={conversation_id}"
            );
            token.cancel();
        }
        self.notify();
        cancelled
    }

    pub fn cancel_conversation(&self, conversation_id: &str) -> usize {
        let n = self.cancel_ids(conversation_id, None).len();
        if n > 0 {
            log::info!(
                "job_supervisor: cancel_conversation conversation_id={conversation_id} jobs={n}"
            );
        }
        n
    }

    /// Claim terminal results for `await`. Already-claimed jobs are skipped (mutex with idle push).
    pub async fn await_jobs(
        &self,
        conversation_id: &str,
        job_ids: Option<Vec<String>>,
        mode: AwaitMode,
        timeout: Option<Duration>,
        parent_cancel: &CancellationToken,
        slot_cap: usize,
    ) -> JobAwaitResult {
        let timeout = timeout.unwrap_or(DEFAULT_AWAIT_TIMEOUT);
        let mut rx = self.subscribe();
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if parent_cancel.is_cancelled() {
                log::info!(
                    "job_supervisor: await cancelled conversation_id={conversation_id} mode={:?}",
                    mode
                );
                return self.snapshot_await(conversation_id, job_ids.as_deref(), mode, false, slot_cap, false);
            }
            if let Some(ready) =
                self.try_claim_await(conversation_id, job_ids.as_deref(), mode, slot_cap)
            {
                return ready;
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                log::info!(
                    "job_supervisor: await timed out conversation_id={conversation_id} mode={:?} (jobs keep running)",
                    mode
                );
                return self.snapshot_await(conversation_id, job_ids.as_deref(), mode, true, slot_cap, false);
            }
            tokio::select! {
                biased;
                _ = parent_cancel.cancelled() => {
                    return self.snapshot_await(
                        conversation_id,
                        job_ids.as_deref(),
                        mode,
                        false,
                        slot_cap,
                        false,
                    );
                }
                _ = tokio::time::sleep(remaining) => {
                    log::info!(
                        "job_supervisor: await timed out conversation_id={conversation_id} mode={:?} (jobs keep running)",
                        mode
                    );
                    return self.snapshot_await(
                        conversation_id,
                        job_ids.as_deref(),
                        mode,
                        true,
                        slot_cap,
                        false,
                    );
                }
                changed = rx.changed() => {
                    if changed.is_err() {
                        return self.snapshot_await(
                            conversation_id,
                            job_ids.as_deref(),
                            mode,
                            false,
                            slot_cap,
                            false,
                        );
                    }
                }
            }
        }
    }

    fn try_claim_await(
        &self,
        conversation_id: &str,
        job_ids: Option<&[String]>,
        mode: AwaitMode,
        slot_cap: usize,
    ) -> Option<JobAwaitResult> {
        let mut inner = self.inner.lock();
        let ids = resolve_job_ids(&inner, conversation_id, job_ids);
        if ids.is_empty() {
            return Some(empty_await(mode, slot_cap, self.running_slot_count()));
        }
        let mut terminal: Vec<String> = Vec::new();
        let mut running: Vec<String> = Vec::new();
        for id in &ids {
            match inner.jobs.get(id) {
                Some(job) if job.conversation_id == conversation_id => {
                    if job.status.is_terminal() {
                        terminal.push(id.clone());
                    } else {
                        running.push(id.clone());
                    }
                }
                _ => {}
            }
        }
        match mode {
            AwaitMode::Any => {
                let claim_id = terminal.iter().find(|id| {
                    inner
                        .jobs
                        .get(*id)
                        .is_some_and(|j| !j.claimed)
                });
                let Some(claim_id) = claim_id.cloned() else {
                    if running.is_empty() {
                        return Some(empty_await(mode, slot_cap, self.running_slot_count()));
                    }
                    return None;
                };
                if let Some(job) = inner.jobs.get_mut(&claim_id) {
                    job.claimed = true;
                    let item = job_await_item(job);
                    log::info!(
                        "job_supervisor: await claimed job_id={claim_id} conversation_id={conversation_id} mode=any"
                    );
                    return Some(JobAwaitResult {
                        mode: "any",
                        timed_out: false,
                        jobs: vec![item],
                        running,
                        running_count: self.running_slot_count(),
                        slot_cap,
                    });
                }
                None
            }
            AwaitMode::All => {
                if !running.is_empty() {
                    return None;
                }
                let mut jobs = Vec::new();
                for id in &terminal {
                    if let Some(job) = inner.jobs.get_mut(id) {
                        job.claimed = true;
                        jobs.push(job_await_item(job));
                    }
                }
                log::info!(
                    "job_supervisor: await claimed count={} conversation_id={conversation_id} mode=all",
                    jobs.len()
                );
                Some(JobAwaitResult {
                    mode: "all",
                    timed_out: false,
                    jobs,
                    running: Vec::new(),
                    running_count: self.running_slot_count(),
                    slot_cap,
                })
            }
        }
    }

    fn snapshot_await(
        &self,
        conversation_id: &str,
        job_ids: Option<&[String]>,
        mode: AwaitMode,
        timed_out: bool,
        slot_cap: usize,
        claim: bool,
    ) -> JobAwaitResult {
        let mut inner = self.inner.lock();
        let ids = resolve_job_ids(&inner, conversation_id, job_ids);
        let mut jobs = Vec::new();
        let mut running = Vec::new();
        for id in ids {
            let Some(job) = inner.jobs.get_mut(&id) else {
                continue;
            };
            if job.conversation_id != conversation_id {
                continue;
            }
            if job.status.is_terminal() {
                if claim {
                    job.claimed = true;
                }
                jobs.push(job_await_item(job));
            } else {
                running.push(id);
            }
        }
        JobAwaitResult {
            mode: match mode {
                AwaitMode::Any => "any",
                AwaitMode::All => "all",
            },
            timed_out,
            jobs,
            running,
            running_count: self.running_slot_count(),
            slot_cap,
        }
    }
}

fn resolve_job_ids(inner: &Inner, conversation_id: &str, job_ids: Option<&[String]>) -> Vec<String> {
    match job_ids {
        Some(ids) if !ids.is_empty() => ids.to_vec(),
        _ => inner
            .by_conversation
            .get(conversation_id)
            .cloned()
            .unwrap_or_default(),
    }
}

fn empty_await(mode: AwaitMode, slot_cap: usize, running_count: usize) -> JobAwaitResult {
    JobAwaitResult {
        mode: match mode {
            AwaitMode::Any => "any",
            AwaitMode::All => "all",
        },
        timed_out: false,
        jobs: Vec::new(),
        running: Vec::new(),
        running_count,
        slot_cap,
    }
}

fn job_list_item(job: &JobRecord, include_content: bool) -> JobListItem {
    let (kind, agent_id, title) = match &job.kind {
        JobKind::Subagent(k) => ("subagent", Some(k.agent_id.clone()), Some(k.title.clone())),
    };
    let content = if include_content && job.status.is_terminal() {
        job.content
            .as_ref()
            .map(|c| crate::text_util::truncate_chars(c, CONTENT_PREVIEW_CHARS))
    } else {
        None
    };
    JobListItem {
        job_id: job.id.clone(),
        status: job.status.as_str(),
        kind,
        agent_id,
        title,
        claimed: job.claimed,
        content,
        error: job.error.clone(),
    }
}

fn job_await_item(job: &JobRecord) -> JobAwaitItem {
    let agent_id = match &job.kind {
        JobKind::Subagent(k) => Some(k.agent_id.clone()),
    };
    JobAwaitItem {
        job_id: job.id.clone(),
        status: job.status.as_str(),
        agent_id,
        content: job.content.clone(),
        error: job.error.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind() -> JobKind {
        JobKind::Subagent(JobKindSubagent {
            tool_call_id: "tc1".into(),
            message_id: "m1".into(),
            agent_id: "explore".into(),
            title: "map auth".into(),
        })
    }

    #[tokio::test]
    async fn await_any_claims_first_terminal_and_leaves_running() {
        let sup = JobSupervisor::new();
        let conv = "c1";
        let a = sup.register(conv, kind(), CancellationToken::new());
        let b = sup.register(conv, kind(), CancellationToken::new());
        sup.mark_running(&a);
        sup.mark_running(&b);
        sup.finish(&a, JobStatus::Completed, Some("{\"content\":\"one\"}".into()), None);

        let parent = CancellationToken::new();
        let result = sup
            .await_jobs(
                conv,
                Some(vec![a.clone(), b.clone()]),
                AwaitMode::Any,
                Some(Duration::from_secs(1)),
                &parent,
                4,
            )
            .await;
        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.jobs[0].job_id, a);
        assert_eq!(result.running, vec![b.clone()]);
        assert!(!result.timed_out);

        let listed = sup.list(conv, false);
        let a_row = listed.iter().find(|j| j.job_id == a).unwrap();
        assert!(a_row.claimed);
        let b_row = listed.iter().find(|j| j.job_id == b).unwrap();
        assert!(!b_row.claimed);
        assert_eq!(b_row.status, "running");
    }

    #[tokio::test]
    async fn await_all_waits_until_every_id_is_terminal() {
        let sup = JobSupervisor::new();
        let conv = "c1";
        let a = sup.register(conv, kind(), CancellationToken::new());
        let b = sup.register(conv, kind(), CancellationToken::new());
        sup.finish(&a, JobStatus::Completed, Some("a".into()), None);

        let parent = CancellationToken::new();
        let sup = std::sync::Arc::new(sup);
        let sup_finish = sup.clone();
        let b_clone = b.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            sup_finish.finish(&b_clone, JobStatus::Completed, Some("b".into()), None);
        });
        let result = sup
            .await_jobs(
                conv,
                Some(vec![a, b]),
                AwaitMode::All,
                Some(Duration::from_secs(2)),
                &parent,
                4,
            )
            .await;
        assert_eq!(result.jobs.len(), 2);
        assert!(result.running.is_empty());
        assert!(!result.timed_out);
    }

    #[tokio::test]
    async fn timeout_does_not_kill_or_claim() {
        let sup = JobSupervisor::new();
        let conv = "c1";
        let a = sup.register(conv, kind(), CancellationToken::new());
        sup.mark_running(&a);
        let parent = CancellationToken::new();
        let result = sup
            .await_jobs(
                conv,
                Some(vec![a.clone()]),
                AwaitMode::All,
                Some(Duration::from_millis(20)),
                &parent,
                4,
            )
            .await;
        assert!(result.timed_out);
        assert_eq!(result.running, vec![a.clone()]);
        assert!(!sup.list(conv, false)[0].claimed);
        assert_eq!(sup.status(conv, &a).unwrap().status, "running");
    }

    #[tokio::test]
    async fn slot_acquire_respects_cap_and_cancel() {
        let sup = JobSupervisor::new();
        assert!(sup.acquire_slot(1, &CancellationToken::new()).await);
        assert_eq!(sup.running_slot_count(), 1);
        let cancel = CancellationToken::new();
        let sup = std::sync::Arc::new(sup);
        let wait = {
            let sup = sup.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move { sup.acquire_slot(1, &cancel).await })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancel.cancel();
        assert!(!wait.await.unwrap());
        sup.release_slot();
        assert_eq!(sup.running_slot_count(), 0);
        assert!(sup.acquire_slot(1, &CancellationToken::new()).await);
        sup.release_slot();
    }

    #[test]
    fn cancel_conversation_signals_tokens() {
        let sup = JobSupervisor::new();
        let token = CancellationToken::new();
        let id = sup.register("c1", kind(), token.clone());
        assert_eq!(sup.cancel_conversation("c1"), 1);
        assert!(token.is_cancelled());
        assert_eq!(sup.cancel_token(&id).unwrap().is_cancelled(), true);
    }
}
