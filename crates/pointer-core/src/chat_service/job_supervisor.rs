//! Background job table for `run_subagent.background` and `terminal.blockUntilMs`.
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
pub struct JobKindTerminal {
    pub tool_call_id: String,
    pub message_id: String,
    pub command: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone)]
pub enum JobKind {
    Subagent(JobKindSubagent),
    Terminal(JobKindTerminal),
}

impl JobKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Subagent(_) => "subagent",
            Self::Terminal(_) => "terminal",
        }
    }
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
    pub kind: &'static str,
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
    /// Timeout / cancel only: finished ids not in `jobs`.
    /// Successful `any` / `all` drain ready bodies into `jobs` instead.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unclaimed: Vec<String>,
    /// Queued + running jobs in **this conversation** (not the global slot counter).
    pub running_count: usize,
    pub slot_cap: usize,
    /// `slotCap - runningCount`. Spawn this many to refill the window.
    pub idle_slots: usize,
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

    /// Conversations that still have queued or running jobs (occupancy UI).
    pub fn occupancy_by_conversation(&self) -> Vec<(String, u32)> {
        let inner = self.inner.lock();
        let mut rows: Vec<(String, u32)> = inner
            .by_conversation
            .iter()
            .filter_map(|(cid, ids)| {
                let n = ids
                    .iter()
                    .filter(|id| {
                        inner
                            .jobs
                            .get(*id)
                            .is_some_and(|j| !j.status.is_terminal())
                    })
                    .count();
                if n == 0 {
                    None
                } else {
                    Some((cid.clone(), n as u32))
                }
            })
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows
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

    pub fn is_claimed(&self, job_id: &str) -> bool {
        self.inner
            .lock()
            .jobs
            .get(job_id)
            .is_some_and(|j| j.claimed)
    }

    /// Mark a terminal job claimed (mutex with idle push). Idempotent.
    /// Returns `None` if the job is missing or still running.
    pub fn claim_if_unclaimed(&self, job_id: &str) -> Option<JobAwaitItem> {
        let mut inner = self.inner.lock();
        let job = inner.jobs.get_mut(job_id)?;
        if !job.status.is_terminal() {
            return None;
        }
        if !job.claimed {
            job.claimed = true;
            log::info!(
                "job_supervisor: claimed job_id={job_id} conversation_id={}",
                job.conversation_id
            );
        }
        Some(job_await_item(job))
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
        Ok(job_list_item(job, false))
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
            return Some(empty_await(
                mode,
                slot_cap,
                running_count_in(&inner, conversation_id),
            ));
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
                let ready = unclaimed_finished_conversation(&inner, conversation_id);
                if ready.is_empty() {
                    if running.is_empty() {
                        return Some(empty_await(
                            mode,
                            slot_cap,
                            running_count_in(&inner, conversation_id),
                        ));
                    }
                    return None;
                }
                let jobs = claim_ready_jobs(&mut inner, &ready);
                let running_count = running_count_in(&inner, conversation_id);
                log::info!(
                    "job_supervisor: await claimed count={} conversation_id={conversation_id} mode=any running_count={running_count}",
                    jobs.len()
                );
                Some(pack_await(
                    "any",
                    false,
                    jobs,
                    running,
                    Vec::new(),
                    running_count,
                    slot_cap,
                ))
            }
            AwaitMode::All => {
                if !running.is_empty() {
                    return None;
                }
                let extra = unclaimed_finished_conversation(&inner, conversation_id);
                let mut ready = Vec::new();
                for id in &terminal {
                    if extra.iter().any(|e| e == id) {
                        ready.push(id.clone());
                    }
                }
                for id in extra {
                    if !ready.iter().any(|r| r == &id) {
                        ready.push(id);
                    }
                }
                let jobs = claim_ready_jobs(&mut inner, &ready);
                let running_count = running_count_in(&inner, conversation_id);
                log::info!(
                    "job_supervisor: await claimed count={} conversation_id={conversation_id} mode=all running_count={running_count}",
                    jobs.len()
                );
                Some(pack_await(
                    "all",
                    false,
                    jobs,
                    Vec::new(),
                    Vec::new(),
                    running_count,
                    slot_cap,
                ))
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
        for id in &ids {
            let Some(job) = inner.jobs.get_mut(id) else {
                continue;
            };
            if job.conversation_id != conversation_id {
                continue;
            }
            if job.status.is_terminal() {
                if job.claimed {
                    continue;
                }
                if claim {
                    job.claimed = true;
                }
                jobs.push(job_await_item(job));
            } else {
                running.push(id.clone());
            }
        }
        let running_count = running_count_in(&inner, conversation_id);
        let delivered: Vec<String> = jobs.iter().map(|j| j.job_id.clone()).collect();
        let unclaimed = unclaimed_excluding(&inner, conversation_id, &delivered);
        pack_await(
            match mode {
                AwaitMode::Any => "any",
                AwaitMode::All => "all",
            },
            timed_out,
            jobs,
            running,
            unclaimed,
            running_count,
            slot_cap,
        )
    }
}

fn running_count_in(inner: &Inner, conversation_id: &str) -> usize {
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

fn unclaimed_finished_in(inner: &Inner, conversation_id: &str, ids: &[String]) -> Vec<String> {
    ids.iter()
        .filter(|id| {
            inner.jobs.get(*id).is_some_and(|j| {
                j.conversation_id == conversation_id && j.status.is_terminal() && !j.claimed
            })
        })
        .cloned()
        .collect()
}

fn unclaimed_finished_conversation(inner: &Inner, conversation_id: &str) -> Vec<String> {
    let Some(ids) = inner.by_conversation.get(conversation_id) else {
        return Vec::new();
    };
    unclaimed_finished_in(inner, conversation_id, ids)
}

fn unclaimed_excluding(inner: &Inner, conversation_id: &str, except: &[String]) -> Vec<String> {
    unclaimed_finished_conversation(inner, conversation_id)
        .into_iter()
        .filter(|id| !except.iter().any(|e| e == id))
        .collect()
}

fn claim_ready_jobs(inner: &mut Inner, ids: &[String]) -> Vec<JobAwaitItem> {
    let mut jobs = Vec::new();
    for id in ids {
        let Some(job) = inner.jobs.get_mut(id) else {
            continue;
        };
        if job.status.is_terminal() && !job.claimed {
            job.claimed = true;
            jobs.push(job_await_item(job));
        }
    }
    jobs
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

fn pack_await(
    mode: &'static str,
    timed_out: bool,
    jobs: Vec<JobAwaitItem>,
    running: Vec<String>,
    unclaimed: Vec<String>,
    running_count: usize,
    slot_cap: usize,
) -> JobAwaitResult {
    JobAwaitResult {
        mode,
        timed_out,
        jobs,
        running,
        unclaimed,
        running_count,
        slot_cap,
        idle_slots: slot_cap.saturating_sub(running_count),
    }
}

fn empty_await(mode: AwaitMode, slot_cap: usize, running_count: usize) -> JobAwaitResult {
    pack_await(
        match mode {
            AwaitMode::Any => "any",
            AwaitMode::All => "all",
        },
        false,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        running_count,
        slot_cap,
    )
}

fn job_list_item(job: &JobRecord, include_content: bool) -> JobListItem {
    let (kind, agent_id, title) = match &job.kind {
        JobKind::Subagent(k) => ("subagent", Some(k.agent_id.clone()), Some(k.title.clone())),
        JobKind::Terminal(k) => (
            "terminal",
            None,
            Some(
                k.label
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| crate::text_util::truncate_chars(&k.command, 80)),
            ),
        ),
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
    let (kind, agent_id) = match &job.kind {
        JobKind::Subagent(k) => ("subagent", Some(k.agent_id.clone())),
        JobKind::Terminal(_) => ("terminal", None),
    };
    JobAwaitItem {
        job_id: job.id.clone(),
        status: job.status.as_str(),
        kind,
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
        assert_eq!(
            result.jobs[0].content.as_deref(),
            Some("{\"content\":\"one\"}")
        );
        assert_eq!(result.running, vec![b.clone()]);
        assert!(result.unclaimed.is_empty());
        assert_eq!(result.running_count, 1);
        assert!(!result.timed_out);

        let listed = sup.list(conv, false);
        let a_row = listed.iter().find(|j| j.job_id == a).unwrap();
        assert!(a_row.claimed);
        let b_row = listed.iter().find(|j| j.job_id == b).unwrap();
        assert!(!b_row.claimed);
        assert_eq!(b_row.status, "running");
    }

    #[tokio::test]
    async fn await_any_drains_all_ready_siblings_with_content() {
        let sup = JobSupervisor::new();
        let conv = "c1";
        let a = sup.register(conv, kind(), CancellationToken::new());
        let b = sup.register(conv, kind(), CancellationToken::new());
        let c = sup.register(conv, kind(), CancellationToken::new());
        sup.mark_running(&a);
        sup.mark_running(&b);
        sup.mark_running(&c);
        sup.finish(&a, JobStatus::Completed, Some("one".into()), None);
        sup.finish(&c, JobStatus::Completed, Some("three".into()), None);

        let parent = CancellationToken::new();
        let result = sup
            .await_jobs(
                conv,
                Some(vec![a.clone(), b.clone(), c.clone()]),
                AwaitMode::Any,
                Some(Duration::from_secs(1)),
                &parent,
                3,
            )
            .await;
        assert_eq!(result.jobs.len(), 2);
        assert_eq!(result.jobs[0].job_id, a);
        assert_eq!(result.jobs[0].content.as_deref(), Some("one"));
        assert_eq!(result.jobs[1].job_id, c);
        assert_eq!(result.jobs[1].content.as_deref(), Some("three"));
        assert_eq!(result.running, vec![b.clone()]);
        assert!(result.unclaimed.is_empty());
        assert_eq!(result.running_count, 1);
        assert_eq!(result.slot_cap, 3);
        assert_eq!(result.idle_slots, 2);
        let listed = sup.list(conv, false);
        assert!(listed.iter().find(|j| j.job_id == a).unwrap().claimed);
        assert!(listed.iter().find(|j| j.job_id == c).unwrap().claimed);
        assert!(!listed.iter().find(|j| j.job_id == b).unwrap().claimed);
    }

    #[tokio::test]
    async fn await_any_drains_ready_outside_wait_set_with_content() {
        let sup = JobSupervisor::new();
        let conv = "c1";
        let a = sup.register(conv, kind(), CancellationToken::new());
        let b = sup.register(conv, kind(), CancellationToken::new());
        sup.mark_running(&a);
        sup.mark_running(&b);
        sup.finish(&a, JobStatus::Completed, Some("done-a".into()), None);

        let parent = CancellationToken::new();
        let started = tokio::time::Instant::now();
        let result = sup
            .await_jobs(
                conv,
                Some(vec![b.clone()]),
                AwaitMode::Any,
                Some(Duration::from_secs(5)),
                &parent,
                3,
            )
            .await;
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.jobs[0].job_id, a);
        assert_eq!(result.jobs[0].content.as_deref(), Some("done-a"));
        assert_eq!(result.running, vec![b.clone()]);
        assert!(result.unclaimed.is_empty());
        assert_eq!(result.running_count, 1);
        assert_eq!(result.idle_slots, 2);
        assert!(!result.timed_out);
        let a_row = sup
            .list(conv, false)
            .into_iter()
            .find(|j| j.job_id == a)
            .unwrap();
        assert!(a_row.claimed);
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
    async fn await_all_drains_finished_outside_wait_set() {
        let sup = JobSupervisor::new();
        let conv = "c1";
        let a = sup.register(conv, kind(), CancellationToken::new());
        let b = sup.register(conv, kind(), CancellationToken::new());
        sup.finish(&a, JobStatus::Completed, Some("done-a".into()), None);
        sup.finish(&b, JobStatus::Completed, Some("done-b".into()), None);
        let parent = CancellationToken::new();
        let result = sup
            .await_jobs(
                conv,
                Some(vec![b.clone()]),
                AwaitMode::All,
                Some(Duration::from_secs(1)),
                &parent,
                3,
            )
            .await;
        assert_eq!(result.jobs.len(), 2);
        assert_eq!(result.jobs[0].job_id, b);
        assert_eq!(result.jobs[0].content.as_deref(), Some("done-b"));
        assert_eq!(result.jobs[1].job_id, a);
        assert_eq!(result.jobs[1].content.as_deref(), Some("done-a"));
        assert!(result.unclaimed.is_empty());
        assert!(!result.timed_out);
    }

    #[tokio::test]
    async fn await_all_omits_already_claimed_content() {
        let sup = JobSupervisor::new();
        let conv = "c1";
        let a = sup.register(conv, kind(), CancellationToken::new());
        let b = sup.register(conv, kind(), CancellationToken::new());
        sup.mark_running(&a);
        sup.mark_running(&b);
        sup.finish(&a, JobStatus::Completed, Some("first".into()), None);

        let parent = CancellationToken::new();
        let first = sup
            .await_jobs(
                conv,
                Some(vec![a.clone(), b.clone()]),
                AwaitMode::Any,
                Some(Duration::from_secs(1)),
                &parent,
                3,
            )
            .await;
        assert_eq!(first.jobs.len(), 1);
        assert_eq!(first.jobs[0].job_id, a);
        assert!(sup.is_claimed(&a));

        sup.finish(&b, JobStatus::Completed, Some("second".into()), None);
        let second = sup
            .await_jobs(
                conv,
                Some(vec![a.clone(), b.clone()]),
                AwaitMode::All,
                Some(Duration::from_secs(1)),
                &parent,
                3,
            )
            .await;
        assert_eq!(second.jobs.len(), 1);
        assert_eq!(second.jobs[0].job_id, b);
        assert_eq!(second.jobs[0].content.as_deref(), Some("second"));
        assert!(second.unclaimed.is_empty());
        assert!(sup.is_claimed(&b));
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

    #[test]
    fn list_distinguishes_terminal_from_subagent() {
        let sup = JobSupervisor::new();
        let sub = sup.register("c1", kind(), CancellationToken::new());
        let term = sup.register(
            "c1",
            JobKind::Terminal(JobKindTerminal {
                tool_call_id: "tc-term".into(),
                message_id: "m1".into(),
                command: "cargo test".into(),
                label: Some("跑测试".into()),
            }),
            CancellationToken::new(),
        );
        let listed = sup.list("c1", false);
        let sub_row = listed.iter().find(|j| j.job_id == sub).unwrap();
        let term_row = listed.iter().find(|j| j.job_id == term).unwrap();
        assert_eq!(sub_row.kind, "subagent");
        assert_eq!(sub_row.agent_id.as_deref(), Some("explore"));
        assert_eq!(term_row.kind, "terminal");
        assert!(term_row.agent_id.is_none());
        assert_eq!(term_row.title.as_deref(), Some("跑测试"));
    }

    #[test]
    fn list_and_status_omit_job_body() {
        let sup = JobSupervisor::new();
        let id = sup.register("c1", kind(), CancellationToken::new());
        sup.finish(
            &id,
            JobStatus::Completed,
            Some("worker handoff markdown".into()),
            None,
        );
        let listed = sup.list("c1", false);
        assert!(listed[0].content.is_none());
        let st = sup.status("c1", &id).unwrap();
        assert!(st.content.is_none());
        assert_eq!(st.status, "completed");
        assert!(!st.claimed);
        let peeked = sup.list("c1", true);
        assert_eq!(
            peeked[0].content.as_deref(),
            Some("worker handoff markdown")
        );
    }

    #[test]
    fn occupancy_by_conversation_skips_terminal_jobs() {
        let sup = JobSupervisor::new();
        let live = sup.register("c1", kind(), CancellationToken::new());
        let done = sup.register("c1", kind(), CancellationToken::new());
        let other = sup.register("c2", kind(), CancellationToken::new());
        sup.mark_running(&live);
        sup.finish(&done, JobStatus::Completed, Some("ok".into()), None);
        let rows = sup.occupancy_by_conversation();
        assert_eq!(rows, vec![("c1".into(), 1), ("c2".into(), 1)]);
        assert_eq!(sup.running_count_for_conversation("c1"), 1);
        let _ = (live, other);
    }

    #[test]
    fn claim_if_unclaimed_skips_running_and_is_idempotent() {
        let sup = JobSupervisor::new();
        let id = sup.register(
            "c1",
            JobKind::Terminal(JobKindTerminal {
                tool_call_id: "tc-term".into(),
                message_id: "m1".into(),
                command: "sleep 10".into(),
                label: None,
            }),
            CancellationToken::new(),
        );
        assert!(sup.claim_if_unclaimed(&id).is_none());
        sup.finish(
            &id,
            JobStatus::Completed,
            Some(r#"{"exitCode":0,"success":true,"stdout":"ok"}"#.into()),
            None,
        );
        let first = sup.claim_if_unclaimed(&id).unwrap();
        assert_eq!(first.kind, "terminal");
        assert!(first.agent_id.is_none());
        assert!(first.content.unwrap().contains("exitCode"));
        let second = sup.claim_if_unclaimed(&id).unwrap();
        assert_eq!(second.status, "completed");
        assert!(sup.is_claimed(&id));
    }
}
