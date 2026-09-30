//! Background job table + **per-conversation worker pool**.
//!
//! Foreground join and background jobs share one FIFO root-slot queue
//! (`maxParallelSubAgents` per conversation). The job table is only for
//! background handles (`job.list` / `await`). Does not hold
//! `session:{conversation}`. Parent `done` does not cancel jobs.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::models::BackgroundJobView;

const DEFAULT_AWAIT_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const CONTENT_PREVIEW_CHARS: usize = 800;
const MAX_MAILBOX: usize = 48;
const PROGRESS_THROTTLE: Duration = Duration::from_secs(2);
const PROGRESS_TEXT_CHARS: usize = 160;

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
    pub agent_instance_id: String,
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
    /// Parent `run_chat` id; used to defer token `finalize_run` until idle.
    run_id: String,
    /// Issuer path from the conversation root, ending with the agent that
    /// spawned this job (root-most ancestor first). Empty = the lead spawned it.
    /// A job is visible to [`JobCaller::Lead`] and to every instance on this
    /// chain, so a worker sees its own jobs plus its descendants' — never its
    /// parent's or its siblings'.
    owner_chain: Vec<String>,
    kind: JobKind,
    status: JobStatus,
    content: Option<String>,
    error: Option<String>,
    claimed: bool,
    cancel: CancellationToken,
    /// Codex-style mailbox: mid-flight progress + status; drained by `await`.
    mailbox: Vec<JobMail>,
    mail_seq: u64,
    last_progress_text: Option<String>,
    last_progress_at: Option<Instant>,
}

impl JobRecord {
    /// Instance that spawned this job. `None` = lead-owned.
    fn direct_owner(&self) -> Option<&str> {
        self.owner_chain.last().map(String::as_str)
    }

    /// Lead-owned jobs are idle-pushed to the lead and visible to every caller.
    fn is_lead_owned(&self) -> bool {
        self.owner_chain.is_empty()
    }

    /// `Lead` sees the whole conversation; an instance sees the jobs it started
    /// plus the ones its descendants started.
    fn visible_to(&self, caller: JobCaller<'_>) -> bool {
        match caller {
            JobCaller::Lead => true,
            JobCaller::Instance(id) => {
                let id = id.trim();
                !id.is_empty() && self.owner_chain.iter().any(|owner| owner == id)
            }
        }
    }
}

/// Who is asking about background jobs.
///
/// The lead owns the conversation and sees every job; a sub-agent instance sees
/// only its own subtree (its own jobs + its descendants'). A worker therefore
/// cannot `await` the job that *is* itself, which is what used to block a
/// background worker until its own timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobCaller<'a> {
    Lead,
    Instance(&'a str),
}

/// Trim and drop empty entries so a malformed chain degrades to lead-owned
/// instead of creating an invisible job.
fn normalize_owner_chain(owner_chain: Vec<String>) -> Vec<String> {
    owner_chain
        .into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect()
}

/// `jobId` the caller may not touch: it exists in this conversation but belongs
/// to another agent's subtree.
fn ownership_error(job_id: &str) -> String {
    format!("jobId `{job_id}` is not owned by this agent or its sub-agents")
}

#[derive(Debug, Clone)]
struct JobMail {
    seq: u64,
    kind: JobMailKind,
    text: String,
    claimed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobMailKind {
    Status,
    Progress,
}

impl JobMailKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Progress => "progress",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobUpdateItem {
    pub job_id: String,
    pub kind: &'static str,
    pub text: String,
    pub seq: u64,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_instance_id: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_instance_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobAwaitResult {
    pub mode: &'static str,
    pub timed_out: bool,
    pub jobs: Vec<JobAwaitItem>,
    /// Mid-flight mailbox drain (Codex-style any-update wake). Does not claim
    /// terminal `content`; call `await` again for finished bodies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub updates: Vec<JobUpdateItem>,
    pub running: Vec<String>,
    /// Timeout / cancel only: finished ids not in `jobs`.
    /// Successful `any` / `all` drain ready bodies into `jobs` instead.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unclaimed: Vec<String>,
    /// Queued + running **background** jobs in this conversation (occupancy UI).
    pub running_count: usize,
    pub slot_cap: usize,
    /// Shared pool free capacity: `slotCap - poolRunning - waiters`.
    /// Includes foreground root leases; not `slotCap - runningCount`.
    pub idle_slots: usize,
    /// Root slots currently held (foreground join + background).
    pub pool_running: usize,
}

/// Result of `job.cancel`: what was cancelled, plus explicit ids the caller is
/// not allowed to touch (another agent's subtree).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobCancelOutcome {
    pub cancelled: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub denied: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IdlePushItem {
    pub job_id: String,
    pub status: &'static str,
    pub kind: &'static str,
    pub title: Option<String>,
    pub agent_id: Option<String>,
    pub content: Option<String>,
    pub error: Option<String>,
    pub agent_instance_id: Option<String>,
}

#[derive(Default)]
struct Inner {
    jobs: HashMap<String, JobRecord>,
    by_conversation: HashMap<String, Vec<String>>,
}

struct WorkerWaiter {
    cancel: CancellationToken,
    waker: tokio::sync::oneshot::Sender<()>,
}

#[derive(Default)]
struct ConversationWorkerPool {
    running_roots: usize,
    waiters: VecDeque<WorkerWaiter>,
}

struct WorkerPoolState {
    by_conv: Mutex<HashMap<String, ConversationWorkerPool>>,
}

impl WorkerPoolState {
    fn stats(&self, conversation_id: &str) -> (usize, usize) {
        let g = self.by_conv.lock();
        g.get(conversation_id)
            .map(|p| (p.running_roots, p.waiters.len()))
            .unwrap_or((0, 0))
    }

    fn try_acquire_immediate(&self, conversation_id: &str, cap: usize) -> bool {
        let cap = cap.max(1);
        let mut g = self.by_conv.lock();
        let pool = g.entry(conversation_id.to_string()).or_default();
        if pool.running_roots < cap {
            pool.running_roots += 1;
            true
        } else {
            false
        }
    }

    fn enqueue_waiter(
        &self,
        conversation_id: &str,
        cancel: CancellationToken,
        waker: tokio::sync::oneshot::Sender<()>,
    ) {
        let mut g = self.by_conv.lock();
        let pool = g.entry(conversation_id.to_string()).or_default();
        pool.waiters.push_back(WorkerWaiter { cancel, waker });
    }

    fn purge_cancelled(&self, conversation_id: &str) {
        let mut g = self.by_conv.lock();
        let Some(pool) = g.get_mut(conversation_id) else {
            return;
        };
        pool.waiters.retain(|w| !w.cancel.is_cancelled());
    }

    fn has_root(&self, conversation_id: &str) -> bool {
        self.by_conv
            .lock()
            .get(conversation_id)
            .is_some_and(|p| p.running_roots > 0)
    }

    fn release_and_pump(&self, conversation_id: &str) {
        let waker = {
            let mut g = self.by_conv.lock();
            let Some(pool) = g.get_mut(conversation_id) else {
                log::warn!(
                    "job_supervisor: release_root missing pool conversation_id={conversation_id}"
                );
                return;
            };
            let mut next = None;
            while let Some(entry) = pool.waiters.pop_front() {
                if entry.cancel.is_cancelled() {
                    continue;
                }
                next = Some(entry.waker);
                break;
            }
            if next.is_some() {
                // Transfer the root to the waiter — leave running_roots unchanged.
                log::info!(
                    "job_supervisor: root transferred conversation_id={conversation_id} running_roots={} waiters={}",
                    pool.running_roots,
                    pool.waiters.len()
                );
            } else if pool.running_roots == 0 {
                log::warn!(
                    "job_supervisor: release_root underflow conversation_id={conversation_id}"
                );
            } else {
                pool.running_roots -= 1;
                log::info!(
                    "job_supervisor: root released conversation_id={conversation_id} running_roots={} waiters={}",
                    pool.running_roots,
                    pool.waiters.len()
                );
            }
            next
        };
        if let Some(waker) = waker {
            let _ = waker.send(());
        }
    }
}

/// RAII root slot in the per-conversation worker pool.
pub struct WorkerLease {
    conversation_id: String,
    pool: Arc<WorkerPoolState>,
    released: bool,
}

impl WorkerLease {
    pub fn conversation_id(&self) -> &str {
        &self.conversation_id
    }

    /// Release early (same as drop). Idempotent.
    pub fn release(mut self) {
        self.release_inner();
    }

    fn release_inner(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        self.pool.release_and_pump(&self.conversation_id);
    }
}

impl Drop for WorkerLease {
    fn drop(&mut self) {
        self.release_inner();
    }
}

/// Nested spawn under an ancestor that already holds a root slot.
/// Does not change `running_roots`.
pub struct NestedLease {
    conversation_id: String,
}

impl NestedLease {
    pub fn conversation_id(&self) -> &str {
        &self.conversation_id
    }
}

/// Worker pool slot held by one sub-agent run.
///
/// First-level workers take a real root slot; deeper workers share an ancestor
/// root. A nested worker whose ancestor root is already gone is promoted to a
/// root slot (D-A4) so the run is never left unaccounted.
pub enum WorkerSlotLease {
    Root(WorkerLease),
    Nested(NestedLease),
}

impl WorkerSlotLease {
    pub fn conversation_id(&self) -> &str {
        match self {
            Self::Root(lease) => lease.conversation_id(),
            Self::Nested(lease) => lease.conversation_id(),
        }
    }
}

/// First-level workers under the lead (`child_spawn_depth == 1`) take a root slot.
/// Deeper nested workers share the ancestor's root (no new slot — avoids N=1 deadlock).
pub fn worker_needs_root_slot(child_spawn_depth: u32) -> bool {
    child_spawn_depth <= 1
}

pub struct JobSupervisor {
    inner: Mutex<Inner>,
    bump: watch::Sender<u64>,
    pool: Arc<WorkerPoolState>,
    /// Same-conversation idle push: conversation id after Completed/Failed.
    on_pushable: Mutex<Option<Arc<dyn Fn(String) + Send + Sync>>>,
    /// Instance ids claimed by a follow-up that has not registered its job yet.
    followup_reserves: Arc<Mutex<HashSet<(String, String)>>>,
    /// In-flight **foreground** children per `(conversation, issuing instance)`
    /// (empty instance = lead). Background children are counted from the job
    /// table; both together form the per-agent fan-out (`maxChildrenPerAgent`).
    foreground_children: Arc<Mutex<HashMap<(String, String), usize>>>,
}

/// `(conversation_id, issuing instance id)`; empty instance id = the lead.
type ChildFanoutKey = (String, String);

/// RAII slot for one live foreground sub-agent child of an issuing instance.
/// Dropping it frees the fan-out slot. `ChildSpawnGuard::drop` is the only
/// release path, so a cancelled or failed child never leaks a slot.
#[derive(Debug)]
pub struct ChildSpawnGuard {
    key: ChildFanoutKey,
    live: Arc<Mutex<HashMap<ChildFanoutKey, usize>>>,
}

impl Drop for ChildSpawnGuard {
    fn drop(&mut self) {
        let mut live = self.live.lock();
        let Some(count) = live.get_mut(&self.key) else {
            return;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            live.remove(&self.key);
        }
    }
}

/// Fan-out key: a blank/absent issuer instance id means the lead.
fn child_fanout_key(conversation_id: &str, issuer_instance_id: Option<&str>) -> ChildFanoutKey {
    (
        conversation_id.to_string(),
        issuer_instance_id
            .map(str::trim)
            .unwrap_or_default()
            .to_string(),
    )
}

/// Non-terminal background jobs this issuer spawned directly.
fn live_background_children_in(inner: &Inner, conversation_id: &str, issuer: &str) -> usize {
    let Some(ids) = inner.by_conversation.get(conversation_id) else {
        return 0;
    };
    ids.iter()
        .filter(|id| {
            inner.jobs.get(*id).is_some_and(|job| {
                if job.status.is_terminal() {
                    return false;
                }
                if issuer.is_empty() {
                    job.is_lead_owned()
                } else {
                    job.direct_owner() == Some(issuer)
                }
            })
        })
        .count()
}

/// Deterministic fan-out refusal. Names the configured limit so the caller can
/// act on it instead of retrying blindly.
fn child_limit_error(
    issuer_instance_id: Option<&str>,
    max_children: usize,
    live: usize,
) -> String {
    let who = match issuer_instance_id.map(str::trim).filter(|id| !id.is_empty()) {
        Some(id) => format!("agent instance `{id}`"),
        None => "the lead agent".to_string(),
    };
    format!(
        "fan-out limit: {who} already has {live} live sub-agent children; maxChildrenPerAgent={max_children}"
    )
}

/// Holds one sub-agent instance until the follow-up job is registered or the run ends.
/// Drop releases the claim.
pub struct FollowupReserve {
    key: (String, String),
    slots: Arc<Mutex<HashSet<(String, String)>>>,
}

impl Drop for FollowupReserve {
    fn drop(&mut self) {
        let removed = self.slots.lock().remove(&self.key);
        if removed {
            log::info!(
                "job_supervisor: released followup reserve conversation_id={} agent_instance_id={}",
                self.key.0,
                self.key.1
            );
        }
    }
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
            pool: Arc::new(WorkerPoolState {
                by_conv: Mutex::new(HashMap::new()),
            }),
            on_pushable: Mutex::new(None),
            followup_reserves: Arc::new(Mutex::new(HashSet::new())),
            foreground_children: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn set_on_pushable(&self, cb: Arc<dyn Fn(String) + Send + Sync>) {
        *self.on_pushable.lock() = Some(cb);
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

    pub fn pool_running_roots(&self, conversation_id: &str) -> usize {
        self.pool.stats(conversation_id).0
    }

    pub fn pool_waiter_len(&self, conversation_id: &str) -> usize {
        self.pool.stats(conversation_id).1
    }

    /// Free capacity in the shared worker pool (foreground + background).
    pub fn idle_slots(&self, conversation_id: &str, cap: usize) -> usize {
        let cap = Self::slot_cap_from(cap);
        let (running, waiting) = self.pool.stats(conversation_id);
        cap.saturating_sub(running.saturating_add(waiting))
    }

    /// Background jobs still queued/running (sidebar occupancy — not pool roots).
    pub fn running_count_for_conversation(&self, conversation_id: &str) -> usize {
        let inner = self.inner.lock();
        running_count_in(&inner, conversation_id)
    }

    /// Non-terminal jobs for the composer list (includes nested background terminals).
    pub fn occupancy_items_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Vec<BackgroundJobView> {
        let inner = self.inner.lock();
        occupancy_items_in(&inner, conversation_id)
    }

    pub fn background_jobs_event(&self, conversation_id: &str) -> crate::models::StreamEvent {
        let jobs = self.occupancy_items_for_conversation(conversation_id);
        crate::models::StreamEvent::BackgroundJobs {
            conversation_id: conversation_id.to_string(),
            running_count: jobs.len() as u32,
            jobs,
        }
    }

    /// Non-terminal background jobs that share this parent `run_id` (token finalize gate).
    pub fn running_count_for_run(&self, run_id: &str) -> usize {
        if run_id.trim().is_empty() {
            return 0;
        }
        let inner = self.inner.lock();
        running_count_for_run_in(&inner, run_id)
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
                    .filter(|id| inner.jobs.get(*id).is_some_and(|j| !j.status.is_terminal()))
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

    /// Acquire a root worker slot for this conversation (FIFO when full).
    /// Returns `None` if cancelled before acquiring.
    pub async fn acquire_root(
        &self,
        conversation_id: &str,
        cap: usize,
        cancel: &CancellationToken,
    ) -> Option<WorkerLease> {
        let cap = Self::slot_cap_from(cap);
        let conversation_id = conversation_id.to_string();
        if cancel.is_cancelled() {
            log::info!(
                "job_supervisor: acquire_root cancelled before wait conversation_id={conversation_id} cap={cap}"
            );
            return None;
        }
        if self.pool.try_acquire_immediate(&conversation_id, cap) {
            log::info!(
                "job_supervisor: root acquired conversation_id={conversation_id} running_roots={} cap={cap}",
                self.pool.stats(&conversation_id).0
            );
            self.notify();
            return Some(WorkerLease {
                conversation_id,
                pool: Arc::clone(&self.pool),
                released: false,
            });
        }

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pool
            .enqueue_waiter(&conversation_id, cancel.clone(), tx);
        log::info!(
            "job_supervisor: root wait conversation_id={conversation_id} cap={cap} waiters={}",
            self.pool.stats(&conversation_id).1
        );
        self.notify();

        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                self.pool.purge_cancelled(&conversation_id);
                log::info!(
                    "job_supervisor: acquire_root cancelled while waiting conversation_id={conversation_id} cap={cap}"
                );
                self.notify();
                None
            }
            res = rx => {
                match res {
                    Ok(()) => {
                        // Slot transferred to us by release_and_pump.
                        if cancel.is_cancelled() {
                            self.pool.release_and_pump(&conversation_id);
                            self.notify();
                            return None;
                        }
                        log::info!(
                            "job_supervisor: root acquired after wait conversation_id={conversation_id} running_roots={} cap={cap}",
                            self.pool.stats(&conversation_id).0
                        );
                        self.notify();
                        Some(WorkerLease {
                            conversation_id,
                            pool: Arc::clone(&self.pool),
                            released: false,
                        })
                    }
                    Err(_) => {
                        log::warn!(
                            "job_supervisor: acquire_root wait closed conversation_id={conversation_id}"
                        );
                        None
                    }
                }
            }
        }
    }

    /// Nested worker: require an ancestor root; do not take a new slot.
    pub fn acquire_nested(&self, conversation_id: &str) -> Result<NestedLease, String> {
        if self.pool.has_root(conversation_id) {
            log::info!(
                "job_supervisor: nested lease conversation_id={conversation_id} running_roots={}",
                self.pool.stats(conversation_id).0
            );
            return Ok(NestedLease {
                conversation_id: conversation_id.to_string(),
            });
        }
        let msg = format!(
            "nested worker requires an ancestor root slot (conversation_id={conversation_id})"
        );
        log::error!("job_supervisor: {msg}");
        Err(msg)
    }

    /// Acquire the pool slot for one worker run.
    ///
    /// `needs_root` = first-level worker (`worker_needs_root_slot`). A nested
    /// worker shares an ancestor root; when that root is already gone (a queued
    /// nested job starting after its spawner finished) it is **promoted to a
    /// root slot** instead of being refused, so the run stays accounted (D-A4).
    /// Returns `None` only when cancelled before a root slot is acquired.
    pub async fn acquire_worker_slot(
        &self,
        conversation_id: &str,
        cap: usize,
        cancel: &CancellationToken,
        needs_root: bool,
    ) -> Option<WorkerSlotLease> {
        if !needs_root {
            if self.pool.has_root(conversation_id) {
                if let Ok(nested) = self.acquire_nested(conversation_id) {
                    return Some(WorkerSlotLease::Nested(nested));
                }
            }
            log::info!(
                "job_supervisor: nested worker promoted to root slot conversation_id={conversation_id} (no ancestor root)"
            );
        }
        self.acquire_root(conversation_id, cap, cancel)
            .await
            .map(WorkerSlotLease::Root)
    }

    /// Live sub-agent children of one issuing instance: its non-terminal
    /// background jobs plus its in-flight foreground joins.
    pub fn live_children_count(
        &self,
        conversation_id: &str,
        issuer_instance_id: Option<&str>,
    ) -> usize {
        let key = child_fanout_key(conversation_id, issuer_instance_id);
        let background = {
            let inner = self.inner.lock();
            live_background_children_in(&inner, conversation_id, &key.1)
        };
        let foreground = self
            .foreground_children
            .lock()
            .get(&key)
            .copied()
            .unwrap_or(0);
        background + foreground
    }

    /// Fan-out check for a spawn that will register a job immediately after
    /// (background spawns are counted from the job table).
    pub fn check_child_spawn(
        &self,
        conversation_id: &str,
        issuer_instance_id: Option<&str>,
        max_children: usize,
    ) -> Result<(), String> {
        let max = max_children.max(1);
        let live = self.live_children_count(conversation_id, issuer_instance_id);
        if live >= max {
            let msg = child_limit_error(issuer_instance_id, max, live);
            log::warn!(
                "job_supervisor: fan-out refused conversation_id={conversation_id} issuer={} live={live} max={max}",
                issuer_instance_id.unwrap_or("lead")
            );
            return Err(msg);
        }
        Ok(())
    }

    /// Reserve one fan-out slot for a **foreground** child. Fails
    /// deterministically when the issuer is already at `max_children`; it never
    /// queues. The returned guard holds the slot until the child finishes.
    pub fn try_begin_child_spawn(
        &self,
        conversation_id: &str,
        issuer_instance_id: Option<&str>,
        max_children: usize,
    ) -> Result<ChildSpawnGuard, String> {
        let max = max_children.max(1);
        let key = child_fanout_key(conversation_id, issuer_instance_id);
        let background = {
            let inner = self.inner.lock();
            live_background_children_in(&inner, conversation_id, &key.1)
        };
        let mut live = self.foreground_children.lock();
        let entry = live.entry(key.clone()).or_insert(0);
        let total = background.saturating_add(*entry);
        if total >= max {
            if *entry == 0 {
                live.remove(&key);
            }
            let msg = child_limit_error(issuer_instance_id, max, total);
            log::warn!(
                "job_supervisor: fan-out refused conversation_id={conversation_id} issuer={} live={total} max={max}",
                issuer_instance_id.unwrap_or("lead")
            );
            return Err(msg);
        }
        *entry += 1;
        let total = total + 1;
        drop(live);
        log::info!(
            "job_supervisor: fan-out slot reserved conversation_id={conversation_id} issuer={} live={total} max={max}",
            issuer_instance_id.unwrap_or("lead")
        );
        Ok(ChildSpawnGuard {
            key,
            live: Arc::clone(&self.foreground_children),
        })
    }

    /// Register a job in `queued`. Caller must spawn work that acquires a slot.
    /// `run_id` ties the job to the parent `run_chat` for deferred token finalize.
    /// `owner_chain` is the issuer's chain ([`JobCaller`] doc); empty = lead-owned.
    pub fn register(
        &self,
        conversation_id: &str,
        kind: JobKind,
        cancel: CancellationToken,
        run_id: &str,
        owner_chain: Vec<String>,
    ) -> String {
        let id = format!("job_{}", uuid::Uuid::new_v4().simple());
        let owner_chain = normalize_owner_chain(owner_chain);
        let owner_log = if owner_chain.is_empty() {
            "lead".to_string()
        } else {
            owner_chain.join(">")
        };
        let record = JobRecord {
            id: id.clone(),
            conversation_id: conversation_id.to_string(),
            run_id: run_id.to_string(),
            owner_chain,
            kind,
            status: JobStatus::Queued,
            content: None,
            error: None,
            claimed: false,
            cancel,
            mailbox: Vec::new(),
            mail_seq: 0,
            last_progress_text: None,
            last_progress_at: None,
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
            "job_supervisor: registered job_id={id} conversation_id={conversation_id} run_id={run_id} owner_chain={owner_log} status=queued"
        );
        self.notify();
        id
    }

    /// Still-queued or running jobs this sub-agent started **directly**.
    /// Finished jobs are omitted; their bodies stay on `job.await`.
    pub fn open_jobs_spawned_by(
        &self,
        conversation_id: &str,
        parent_agent_instance_id: &str,
    ) -> Vec<OpenBackgroundJob> {
        let parent = parent_agent_instance_id.trim();
        if parent.is_empty() {
            log::warn!(
                "job_supervisor: open jobs lookup skipped empty parent conversation_id={conversation_id}"
            );
            return Vec::new();
        }
        let inner = self.inner.lock();
        let Some(ids) = inner.by_conversation.get(conversation_id) else {
            return Vec::new();
        };
        ids.iter()
            .filter_map(|id| {
                let job = inner.jobs.get(id)?;
                if job.direct_owner() != Some(parent) {
                    return None;
                }
                if job.status.is_terminal() {
                    return None;
                }
                Some(open_background_job(job))
            })
            .collect()
    }

    /// True when this sub-agent thread has a queued/running job or a follow-up claim.
    pub fn subagent_instance_busy(&self, conversation_id: &str, instance_id: &str) -> bool {
        let instance_id = instance_id.trim();
        if instance_id.is_empty() {
            return false;
        }
        if instance_has_live_job(&self.inner.lock(), conversation_id, instance_id) {
            return true;
        }
        self.followup_reserves
            .lock()
            .contains(&(conversation_id.to_string(), instance_id.to_string()))
    }

    /// Claim this instance before loading its transcript. The second caller gets `None`.
    pub fn try_reserve_followup(
        &self,
        conversation_id: &str,
        instance_id: &str,
    ) -> Option<FollowupReserve> {
        let instance_id = instance_id.trim();
        if instance_id.is_empty() {
            return None;
        }
        if instance_has_live_job(&self.inner.lock(), conversation_id, instance_id) {
            log::info!(
                "job_supervisor: followup reserve refused; job live conversation_id={conversation_id} agent_instance_id={instance_id}"
            );
            return None;
        }
        let key = (conversation_id.to_string(), instance_id.to_string());
        let mut slots = self.followup_reserves.lock();
        if !slots.insert(key.clone()) {
            log::info!(
                "job_supervisor: followup reserve refused; already claimed conversation_id={conversation_id} agent_instance_id={instance_id}"
            );
            return None;
        }
        log::info!(
            "job_supervisor: reserved followup conversation_id={conversation_id} agent_instance_id={instance_id}"
        );
        Some(FollowupReserve {
            key,
            slots: Arc::clone(&self.followup_reserves),
        })
    }

    pub fn cancel_token(&self, job_id: &str) -> Option<CancellationToken> {
        self.inner.lock().jobs.get(job_id).map(|j| j.cancel.clone())
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
        push_mail(job, JobMailKind::Status, "running".into());
        log::info!(
            "job_supervisor: job_id={job_id} conversation_id={} status=running",
            job.conversation_id
        );
        drop(inner);
        self.notify();
    }

    /// Record mid-flight progress mail. Does **not** wake `await`
    /// (`mode=any` waits for an unclaimed terminal job only).
    /// Does **not** set job-level `claimed` (terminal bodies stay for later `await` / idle push).
    pub fn post_progress(&self, job_id: &str, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        let text = crate::text_util::truncate_chars(trimmed, PROGRESS_TEXT_CHARS);
        let mut inner = self.inner.lock();
        let Some(job) = inner.jobs.get_mut(job_id) else {
            log::warn!("job_supervisor: post_progress unknown job_id={job_id}");
            return;
        };
        if job.status.is_terminal() {
            return;
        }
        let now = Instant::now();
        if job.last_progress_text.as_deref() == Some(text.as_str()) {
            if job
                .last_progress_at
                .is_some_and(|t| now.duration_since(t) < PROGRESS_THROTTLE)
            {
                return;
            }
        }
        job.last_progress_text = Some(text.clone());
        job.last_progress_at = Some(now);
        push_mail(job, JobMailKind::Progress, text);
        let conversation_id = job.conversation_id.clone();
        log::info!(
            "job_supervisor: progress job_id={job_id} conversation_id={conversation_id} mail_seq={}",
            job.mail_seq
        );
        drop(inner);
        // Do not notify: inner-tool progress must not complete `job.await`.
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

    /// Mark terminal. Returns `Some(run_id)` when this call newly transitions the job
    /// (callers use it to try deferred token finalize).
    pub fn finish(
        &self,
        job_id: &str,
        status: JobStatus,
        content: Option<String>,
        error: Option<String>,
    ) -> Option<String> {
        if !status.is_terminal() {
            log::warn!(
                "job_supervisor: finish ignored non-terminal status={:?} job_id={job_id}",
                status
            );
            return None;
        }
        let mut inner = self.inner.lock();
        let Some(job) = inner.jobs.get_mut(job_id) else {
            log::warn!("job_supervisor: finish unknown job_id={job_id}");
            return None;
        };
        if job.status.is_terminal() {
            log::info!(
                "job_supervisor: finish idempotent job_id={job_id} status={:?}",
                job.status
            );
            return None;
        }
        job.status = status;
        job.content = content;
        job.error = error;
        let conversation_id = job.conversation_id.clone();
        let run_id = job.run_id.clone();
        let lead_owned = job.is_lead_owned();
        let direct_owner = job.direct_owner().map(str::to_string);
        let pushable = lead_owned && matches!(status, JobStatus::Completed | JobStatus::Failed);
        log::info!(
            "job_supervisor: job_id={job_id} conversation_id={} run_id={} status={} lead_owned={lead_owned}",
            conversation_id,
            run_id,
            status.as_str()
        );
        drop(inner);
        self.notify();
        if pushable {
            if let Some(cb) = self.on_pushable.lock().clone() {
                cb(conversation_id);
            }
        } else if matches!(status, JobStatus::Completed | JobStatus::Failed) {
            log::info!(
                "job_supervisor: completion stays with spawning agent job_id={job_id} direct_owner={} status={}",
                direct_owner.as_deref().unwrap_or(""),
                status.as_str()
            );
        }
        Some(run_id)
    }

    /// Claim Completed/Failed unclaimed jobs for idle push. Skips Cancelled.
    pub fn claim_pushable(&self, conversation_id: &str) -> Vec<IdlePushItem> {
        let mut inner = self.inner.lock();
        let ids = inner
            .by_conversation
            .get(conversation_id)
            .cloned()
            .unwrap_or_default();
        let ready: Vec<String> = ids
            .into_iter()
            .filter(|id| inner.jobs.get(id).is_some_and(job_is_idle_pushable))
            .collect();
        if ready.is_empty() {
            return Vec::new();
        }
        let items: Vec<IdlePushItem> = ready
            .iter()
            .filter_map(|id| inner.jobs.get(id).map(idle_push_item))
            .collect();
        for id in &ready {
            if let Some(job) = inner.jobs.get_mut(id) {
                job.claimed = true;
            }
        }
        log::info!(
            "job_supervisor: idle push claimed count={} conversation_id={conversation_id}",
            items.len()
        );
        items
    }

    pub fn unclaim(&self, job_ids: &[String]) {
        let mut inner = self.inner.lock();
        for id in job_ids {
            if let Some(job) = inner.jobs.get_mut(id) {
                if job.status.is_terminal() && job.claimed {
                    job.claimed = false;
                    log::warn!("job_supervisor: unclaimed after failed idle push job_id={id}");
                }
            }
        }
    }

    pub fn list(
        &self,
        conversation_id: &str,
        include_content: bool,
        caller: JobCaller<'_>,
    ) -> Vec<JobListItem> {
        let inner = self.inner.lock();
        let Some(ids) = inner.by_conversation.get(conversation_id) else {
            return Vec::new();
        };
        ids.iter()
            .filter_map(|id| {
                let job = inner.jobs.get(id)?;
                if !job.visible_to(caller) {
                    return None;
                }
                Some(job_list_item(job, include_content))
            })
            .collect()
    }

    pub fn status(
        &self,
        conversation_id: &str,
        job_id: &str,
        caller: JobCaller<'_>,
    ) -> Result<JobListItem, String> {
        let inner = self.inner.lock();
        let Some(job) = inner.jobs.get(job_id) else {
            return Err(format!("unknown jobId `{job_id}`"));
        };
        if job.conversation_id != conversation_id {
            return Err("jobId does not belong to this conversation".into());
        }
        if !job.visible_to(caller) {
            return Err(ownership_error(job_id));
        }
        Ok(job_list_item(job, false))
    }

    /// Cancel background jobs. `None`/empty `job_ids` = every job visible to
    /// `caller` (a sub-agent never cancels the lead's or a sibling's work).
    /// Explicit ids outside the caller's subtree are skipped and reported in
    /// [`JobCancelOutcome::denied`]; unknown ids stay silent.
    pub fn cancel_ids(
        &self,
        conversation_id: &str,
        job_ids: Option<&[String]>,
        caller: JobCaller<'_>,
    ) -> JobCancelOutcome {
        let mut denied = Vec::new();
        let tokens: Vec<(String, CancellationToken)> = {
            let inner = self.inner.lock();
            let ids: Vec<String> = match job_ids {
                Some(ids) if !ids.is_empty() => ids
                    .iter()
                    .filter(|id| match inner.jobs.get(id.as_str()) {
                        None => false,
                        Some(job) if job.visible_to(caller) => true,
                        Some(_) => {
                            denied.push((*id).clone());
                            false
                        }
                    })
                    .cloned()
                    .collect(),
                _ => visible_job_ids(&inner, conversation_id, caller),
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
        let cascade = self.cascade_cancel_tokens(conversation_id, &tokens, caller);
        let cancelled: Vec<String> = tokens
            .iter()
            .chain(cascade.iter())
            .map(|(id, _)| id.clone())
            .collect();
        for (id, token) in tokens.iter().chain(cascade.iter()) {
            log::info!("job_supervisor: cancelling job_id={id} conversation_id={conversation_id}");
            token.cancel();
        }
        if !denied.is_empty() {
            log::warn!(
                "job_supervisor: cancel denied for foreign jobs conversation_id={conversation_id} denied={denied:?}"
            );
        }
        self.notify();
        JobCancelOutcome { cancelled, denied }
    }

    /// Jobs to cancel on top of `tokens`: cancelling a sub-agent job cancels its
    /// whole subtree, because every descendant job's `owner_chain` contains the
    /// cancelled job's own instance id. Still filtered by `visible_to(caller)`
    /// so the P1 owner-chain scope is never widened.
    fn cascade_cancel_tokens(
        &self,
        conversation_id: &str,
        tokens: &[(String, CancellationToken)],
        caller: JobCaller<'_>,
    ) -> Vec<(String, CancellationToken)> {
        let inner = self.inner.lock();
        let seeds: Vec<String> = tokens
            .iter()
            .filter_map(|(id, _)| match inner.jobs.get(id).map(|job| &job.kind) {
                Some(JobKind::Subagent(kind)) => Some(kind.agent_instance_id.clone()),
                _ => None,
            })
            .filter(|id| !id.trim().is_empty())
            .collect();
        if seeds.is_empty() {
            return Vec::new();
        }
        let Some(ids) = inner.by_conversation.get(conversation_id) else {
            return Vec::new();
        };
        let cascade: Vec<(String, CancellationToken)> = ids
            .iter()
            .filter(|id| !tokens.iter().any(|(cancelled, _)| cancelled == *id))
            .filter_map(|id| {
                let job = inner.jobs.get(id)?;
                if job.status.is_terminal() || !job.visible_to(caller) {
                    return None;
                }
                if !job
                    .owner_chain
                    .iter()
                    .any(|owner| seeds.iter().any(|seed| seed == owner))
                {
                    return None;
                }
                Some((id.clone(), job.cancel.clone()))
            })
            .collect();
        if !cascade.is_empty() {
            log::info!(
                "job_supervisor: cancel cascade conversation_id={conversation_id} roots={} subtree={}",
                tokens.len(),
                cascade.len()
            );
        }
        cascade
    }

    pub fn cancel_conversation(&self, conversation_id: &str) -> usize {
        let n = self
            .cancel_ids(conversation_id, None, JobCaller::Lead)
            .cancelled
            .len();
        if n > 0 {
            log::info!(
                "job_supervisor: cancel_conversation conversation_id={conversation_id} jobs={n}"
            );
        }
        n
    }

    /// Claim terminal results and drain leftover mailbox for `await`.
    /// `mode=any` wakes only on an unclaimed terminal job (not inner-tool progress).
    /// Already-claimed jobs are skipped (mutex with idle push).
    ///
    /// Explicit `jobIds` outside the caller's subtree are a hard error: the
    /// caller must not silently wait on someone else's work.
    pub async fn await_jobs(
        &self,
        conversation_id: &str,
        caller: JobCaller<'_>,
        job_ids: Option<Vec<String>>,
        mode: AwaitMode,
        timeout: Option<Duration>,
        parent_cancel: &CancellationToken,
        slot_cap: usize,
    ) -> Result<JobAwaitResult, String> {
        let job_ids = self.visible_await_ids(conversation_id, caller, job_ids)?;
        let timeout = timeout.unwrap_or(DEFAULT_AWAIT_TIMEOUT);
        let mut rx = self.subscribe();
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if parent_cancel.is_cancelled() {
                log::info!(
                    "job_supervisor: await cancelled conversation_id={conversation_id} mode={:?}",
                    mode
                );
                return Ok(self.snapshot_await(
                    conversation_id,
                    job_ids.as_deref(),
                    mode,
                    false,
                    slot_cap,
                    caller,
                ));
            }
            if let Some(ready) =
                self.try_claim_await(conversation_id, job_ids.as_deref(), mode, slot_cap, caller)
            {
                return Ok(ready);
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                log::info!(
                    "job_supervisor: await timed out conversation_id={conversation_id} mode={:?} (jobs keep running)",
                    mode
                );
                return Ok(self.snapshot_await(
                    conversation_id,
                    job_ids.as_deref(),
                    mode,
                    true,
                    slot_cap,
                    caller,
                ));
            }
            tokio::select! {
                biased;
                _ = parent_cancel.cancelled() => {
                    return Ok(self.snapshot_await(
                        conversation_id,
                        job_ids.as_deref(),
                        mode,
                        false,
                        slot_cap,
                        caller,
                    ));
                }
                _ = tokio::time::sleep(remaining) => {
                    log::info!(
                        "job_supervisor: await timed out conversation_id={conversation_id} mode={:?} (jobs keep running)",
                        mode
                    );
                    return Ok(self.snapshot_await(
                        conversation_id,
                        job_ids.as_deref(),
                        mode,
                        true,
                        slot_cap,
                        caller,
                    ));
                }
                changed = rx.changed() => {
                    if changed.is_err() {
                        return Ok(self.snapshot_await(
                            conversation_id,
                            job_ids.as_deref(),
                            mode,
                            false,
                            slot_cap,
                            caller,
                        ));
                    }
                }
            }
        }
    }

    /// Explicit `jobIds` must be visible to the caller. An id that exists in
    /// this conversation but belongs to another subtree is rejected; an unknown
    /// id keeps the historical silent behaviour (it simply matches nothing).
    fn visible_await_ids(
        &self,
        conversation_id: &str,
        caller: JobCaller<'_>,
        job_ids: Option<Vec<String>>,
    ) -> Result<Option<Vec<String>>, String> {
        let Some(ids) = job_ids.filter(|ids| !ids.is_empty()) else {
            return Ok(None);
        };
        {
            let inner = self.inner.lock();
            for id in &ids {
                let foreign = inner.jobs.get(id.as_str()).is_some_and(|job| {
                    job.conversation_id == conversation_id && !job.visible_to(caller)
                });
                if foreign {
                    log::warn!(
                        "job_supervisor: await rejected foreign jobId conversation_id={conversation_id} job_id={id}"
                    );
                    return Err(ownership_error(id));
                }
            }
        }
        Ok(Some(ids))
    }

    fn try_claim_await(
        &self,
        conversation_id: &str,
        job_ids: Option<&[String]>,
        mode: AwaitMode,
        slot_cap: usize,
        caller: JobCaller<'_>,
    ) -> Option<JobAwaitResult> {
        let idle = self.idle_slots(conversation_id, slot_cap);
        let pool_running = self.pool_running_roots(conversation_id);
        let mut inner = self.inner.lock();
        let ids = resolve_job_ids(&inner, conversation_id, job_ids, caller);
        if ids.is_empty() {
            return Some(empty_await(
                mode,
                slot_cap,
                running_count_in(&inner, conversation_id),
                idle,
                pool_running,
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
                let ready = unclaimed_finished_conversation(&inner, conversation_id, caller);
                if ready.is_empty() {
                    if running.is_empty() {
                        return Some(empty_await(
                            mode,
                            slot_cap,
                            running_count_in(&inner, conversation_id),
                            idle,
                            pool_running,
                        ));
                    }
                    return None;
                }
                let jobs = claim_ready_jobs(&mut inner, &ready);
                let updates = drain_mail_for_ids(&mut inner, conversation_id, &ids);
                let running_count = running_count_in(&inner, conversation_id);
                log::info!(
                    "job_supervisor: await claimed count={} updates={} conversation_id={conversation_id} mode=any running_count={running_count}",
                    jobs.len(),
                    updates.len()
                );
                Some(pack_await(
                    "any",
                    false,
                    jobs,
                    updates,
                    running,
                    Vec::new(),
                    running_count,
                    slot_cap,
                    idle,
                    pool_running,
                ))
            }
            AwaitMode::All => {
                if !running.is_empty() {
                    return None;
                }
                let extra = unclaimed_finished_conversation(&inner, conversation_id, caller);
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
                let updates = drain_mail_for_ids(&mut inner, conversation_id, &ids);
                let running_count = running_count_in(&inner, conversation_id);
                log::info!(
                    "job_supervisor: await claimed count={} updates={} conversation_id={conversation_id} mode=all running_count={running_count}",
                    jobs.len(),
                    updates.len()
                );
                Some(pack_await(
                    "all",
                    false,
                    jobs,
                    updates,
                    Vec::new(),
                    Vec::new(),
                    running_count,
                    slot_cap,
                    idle,
                    pool_running,
                ))
            }
        }
    }

    /// Timeout / cancel / watch-closed path: **never** deliver `content` and **never** claim.
    /// Finished-but-unclaimed ids go in `unclaimed`; still-running ids go in `running`.
    /// Leftover mail is drained into `updates` (no body claim).
    /// Only [`Self::try_claim_await`] may put bodies into `jobs`.
    fn snapshot_await(
        &self,
        conversation_id: &str,
        job_ids: Option<&[String]>,
        mode: AwaitMode,
        timed_out: bool,
        slot_cap: usize,
        caller: JobCaller<'_>,
    ) -> JobAwaitResult {
        let idle = self.idle_slots(conversation_id, slot_cap);
        let pool_running = self.pool_running_roots(conversation_id);
        let mut inner = self.inner.lock();
        let ids = resolve_job_ids(&inner, conversation_id, job_ids, caller);
        let mut running = Vec::new();
        for id in &ids {
            let Some(job) = inner.jobs.get(id) else {
                continue;
            };
            if job.conversation_id != conversation_id {
                continue;
            }
            if !job.status.is_terminal() {
                running.push(id.clone());
            }
        }
        let running_count = running_count_in(&inner, conversation_id);
        let unclaimed = unclaimed_finished_conversation(&inner, conversation_id, caller);
        let updates = drain_mail_for_ids(&mut inner, conversation_id, &ids);
        pack_await(
            match mode {
                AwaitMode::Any => "any",
                AwaitMode::All => "all",
            },
            timed_out,
            Vec::new(),
            updates,
            running,
            unclaimed,
            running_count,
            slot_cap,
            idle,
            pool_running,
        )
    }
}

fn running_count_in(inner: &Inner, conversation_id: &str) -> usize {
    inner
        .by_conversation
        .get(conversation_id)
        .map(|ids| {
            ids.iter()
                .filter(|id| inner.jobs.get(*id).is_some_and(|j| !j.status.is_terminal()))
                .count()
        })
        .unwrap_or(0)
}

fn occupancy_items_in(inner: &Inner, conversation_id: &str) -> Vec<BackgroundJobView> {
    let Some(ids) = inner.by_conversation.get(conversation_id) else {
        return Vec::new();
    };
    ids.iter()
        .filter_map(|id| inner.jobs.get(id))
        .filter(|job| !job.status.is_terminal())
        .map(occupancy_item)
        .collect()
}

fn occupancy_item(job: &JobRecord) -> BackgroundJobView {
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
    BackgroundJobView {
        job_id: job.id.clone(),
        status: job.status.as_str().to_string(),
        kind: kind.to_string(),
        title,
        agent_id,
    }
}

fn running_count_for_run_in(inner: &Inner, run_id: &str) -> usize {
    inner
        .jobs
        .values()
        .filter(|j| j.run_id == run_id && !j.status.is_terminal())
        .count()
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

fn unclaimed_finished_conversation(
    inner: &Inner,
    conversation_id: &str,
    caller: JobCaller<'_>,
) -> Vec<String> {
    let Some(ids) = inner.by_conversation.get(conversation_id) else {
        return Vec::new();
    };
    let ids: Vec<String> = ids
        .iter()
        .filter(|id| inner.jobs.get(*id).is_some_and(|job| job.visible_to(caller)))
        .cloned()
        .collect();
    unclaimed_finished_in(inner, conversation_id, &ids)
}

/// Every job in this conversation the caller may see.
fn visible_job_ids(inner: &Inner, conversation_id: &str, caller: JobCaller<'_>) -> Vec<String> {
    inner
        .by_conversation
        .get(conversation_id)
        .map(|ids| {
            ids.iter()
                .filter(|id| inner.jobs.get(*id).is_some_and(|job| job.visible_to(caller)))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
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

fn push_mail(job: &mut JobRecord, kind: JobMailKind, text: String) {
    job.mail_seq = job.mail_seq.saturating_add(1);
    job.mailbox.push(JobMail {
        seq: job.mail_seq,
        kind,
        text,
        claimed: false,
    });
    while job.mailbox.len() > MAX_MAILBOX {
        if let Some(idx) = job.mailbox.iter().position(|m| m.claimed) {
            job.mailbox.remove(idx);
        } else {
            job.mailbox.remove(0);
        }
    }
}

fn drain_mail_for_ids(
    inner: &mut Inner,
    conversation_id: &str,
    ids: &[String],
) -> Vec<JobUpdateItem> {
    let mut updates = Vec::new();
    for id in ids {
        let Some(job) = inner.jobs.get_mut(id) else {
            continue;
        };
        if job.conversation_id != conversation_id {
            continue;
        }
        for mail in job.mailbox.iter_mut() {
            if mail.claimed {
                continue;
            }
            mail.claimed = true;
            updates.push(JobUpdateItem {
                job_id: job.id.clone(),
                kind: mail.kind.as_str(),
                text: mail.text.clone(),
                seq: mail.seq,
            });
        }
    }
    updates.sort_by(|a, b| a.seq.cmp(&b.seq).then_with(|| a.job_id.cmp(&b.job_id)));
    updates
}

fn resolve_job_ids(
    inner: &Inner,
    conversation_id: &str,
    job_ids: Option<&[String]>,
    caller: JobCaller<'_>,
) -> Vec<String> {
    match job_ids {
        Some(ids) if !ids.is_empty() => ids
            .iter()
            .filter(|id| {
                inner.jobs.get(id.as_str()).is_some_and(|job| {
                    job.conversation_id == conversation_id && job.visible_to(caller)
                })
            })
            .cloned()
            .collect(),
        _ => visible_job_ids(inner, conversation_id, caller),
    }
}

fn pack_await(
    mode: &'static str,
    timed_out: bool,
    jobs: Vec<JobAwaitItem>,
    updates: Vec<JobUpdateItem>,
    running: Vec<String>,
    unclaimed: Vec<String>,
    running_count: usize,
    slot_cap: usize,
    idle_slots: usize,
    pool_running: usize,
) -> JobAwaitResult {
    JobAwaitResult {
        mode,
        timed_out,
        jobs,
        updates,
        running,
        unclaimed,
        running_count,
        slot_cap,
        idle_slots,
        pool_running,
    }
}

fn empty_await(
    mode: AwaitMode,
    slot_cap: usize,
    running_count: usize,
    idle_slots: usize,
    pool_running: usize,
) -> JobAwaitResult {
    pack_await(
        match mode {
            AwaitMode::Any => "any",
            AwaitMode::All => "all",
        },
        false,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        running_count,
        slot_cap,
        idle_slots,
        pool_running,
    )
}

fn instance_has_live_job(inner: &Inner, conversation_id: &str, instance_id: &str) -> bool {
    let Some(ids) = inner.by_conversation.get(conversation_id) else {
        return false;
    };
    ids.iter().any(|id| {
        let Some(job) = inner.jobs.get(id) else {
            return false;
        };
        if job.status.is_terminal() {
            return false;
        }
        match &job.kind {
            JobKind::Subagent(kind) => kind.agent_instance_id == instance_id,
            JobKind::Terminal(_) => false,
        }
    })
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
        agent_instance_id: match &job.kind {
            JobKind::Subagent(k) => Some(k.agent_instance_id.clone()).filter(|s| !s.is_empty()),
            JobKind::Terminal(_) => None,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenBackgroundJob {
    pub job_id: String,
    pub status: String,
    pub kind: String,
    pub title: Option<String>,
}

fn open_background_job(job: &JobRecord) -> OpenBackgroundJob {
    let (kind, title) = match &job.kind {
        JobKind::Subagent(k) => (
            "subagent",
            Some(k.title.clone()).filter(|s| !s.trim().is_empty()),
        ),
        JobKind::Terminal(k) => (
            "terminal",
            Some(
                k.label
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| crate::text_util::truncate_chars(&k.command, 80)),
            ),
        ),
    };
    OpenBackgroundJob {
        job_id: job.id.clone(),
        status: job.status.as_str().to_string(),
        kind: kind.to_string(),
        title,
    }
}

fn job_is_idle_pushable(job: &JobRecord) -> bool {
    job.is_lead_owned()
        && !job.claimed
        && matches!(job.status, JobStatus::Completed | JobStatus::Failed)
}

fn idle_push_item(job: &JobRecord) -> IdlePushItem {
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
    IdlePushItem {
        job_id: job.id.clone(),
        status: job.status.as_str(),
        kind,
        title,
        agent_id,
        content: job.content.clone(),
        error: job.error.clone(),
        agent_instance_id: match &job.kind {
            JobKind::Subagent(k) => Some(k.agent_instance_id.clone()).filter(|s| !s.is_empty()),
            JobKind::Terminal(_) => None,
        },
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
        agent_instance_id: match &job.kind {
            JobKind::Subagent(k) => Some(k.agent_instance_id.clone()).filter(|s| !s.is_empty()),
            JobKind::Terminal(_) => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn kind() -> JobKind {
        JobKind::Subagent(JobKindSubagent {
            tool_call_id: "tc1".into(),
            message_id: "m1".into(),
            agent_id: "explore".into(),
            title: "map auth".into(),
            agent_instance_id: "inst-job".into(),
        })
    }

    #[tokio::test]
    async fn await_any_claims_first_terminal_and_leaves_running() {
        let sup = JobSupervisor::new();
        let conv = "c1";
        let a = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        let b = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        sup.mark_running(&a);
        sup.mark_running(&b);
        sup.finish(
            &a,
            JobStatus::Completed,
            Some("{\"content\":\"one\"}".into()),
            None,
        );

        let parent = CancellationToken::new();
        let result = sup
            .await_jobs(
                conv,
                JobCaller::Lead,
                Some(vec![a.clone(), b.clone()]),
                AwaitMode::Any,
                Some(Duration::from_secs(1)),
                &parent,
                4,
                        )
            .await
            .expect("lead await");
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

        let listed = sup.list(conv, false, JobCaller::Lead);
        let a_row = listed.iter().find(|j| j.job_id == a).unwrap();
        assert!(a_row.claimed);
        let b_row = listed.iter().find(|j| j.job_id == b).unwrap();
        assert!(!b_row.claimed);
        assert_eq!(b_row.status, "running");
    }

    #[tokio::test]
    async fn await_any_does_not_wake_on_inner_tool_progress() {
        let sup = Arc::new(JobSupervisor::new());
        let conv = "c-progress";
        let a = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        let b = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        sup.mark_running(&a);
        sup.mark_running(&b);
        sup.post_progress(&a, "read · src/auth.rs");

        let parent = CancellationToken::new();
        let timed = Arc::clone(&sup)
            .await_jobs(
                conv,
                JobCaller::Lead,
                Some(vec![a.clone(), b.clone()]),
                AwaitMode::Any,
                Some(Duration::from_millis(80)),
                &parent,
                4,
                        )
            .await
            .expect("lead await");
        assert!(
            timed.timed_out,
            "inner-tool progress must not complete await"
        );
        assert!(timed.jobs.is_empty());
        assert_eq!(timed.running, vec![a.clone(), b.clone()]);
        assert!(
            !sup.list(conv, false, JobCaller::Lead)
                .iter()
                .find(|j| j.job_id == a)
                .unwrap()
                .claimed
        );

        sup.finish(&a, JobStatus::Completed, Some("handoff".into()), None);
        let done = Arc::clone(&sup)
            .await_jobs(
                conv,
                JobCaller::Lead,
                Some(vec![a.clone(), b.clone()]),
                AwaitMode::Any,
                Some(Duration::from_secs(1)),
                &parent,
                4,
                        )
            .await
            .expect("lead await");
        assert!(!done.timed_out);
        assert_eq!(done.jobs.len(), 1);
        assert_eq!(done.jobs[0].job_id, a);
        assert_eq!(done.jobs[0].content.as_deref(), Some("handoff"));
        assert_eq!(done.running, vec![b.clone()]);
        assert!(sup.is_claimed(&a));
    }

    #[tokio::test]
    async fn await_any_drains_all_ready_siblings_with_content() {
        let sup = JobSupervisor::new();
        let conv = "c1";
        let a = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        let b = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        let c = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        sup.mark_running(&a);
        sup.mark_running(&b);
        sup.mark_running(&c);
        sup.finish(&a, JobStatus::Completed, Some("one".into()), None);
        sup.finish(&c, JobStatus::Completed, Some("three".into()), None);

        let parent = CancellationToken::new();
        let result = sup
            .await_jobs(
                conv,
                JobCaller::Lead,
                Some(vec![a.clone(), b.clone(), c.clone()]),
                AwaitMode::Any,
                Some(Duration::from_secs(1)),
                &parent,
                3,
                        )
            .await
            .expect("lead await");
        assert_eq!(result.jobs.len(), 2);
        assert_eq!(result.jobs[0].job_id, a);
        assert_eq!(result.jobs[0].content.as_deref(), Some("one"));
        assert_eq!(result.jobs[1].job_id, c);
        assert_eq!(result.jobs[1].content.as_deref(), Some("three"));
        assert_eq!(result.running, vec![b.clone()]);
        assert!(result.unclaimed.is_empty());
        assert_eq!(result.running_count, 1);
        assert_eq!(result.slot_cap, 3);
        assert_eq!(result.idle_slots, 3);
        assert_eq!(result.pool_running, 0);
        let listed = sup.list(conv, false, JobCaller::Lead);
        assert!(listed.iter().find(|j| j.job_id == a).unwrap().claimed);
        assert!(listed.iter().find(|j| j.job_id == c).unwrap().claimed);
        assert!(!listed.iter().find(|j| j.job_id == b).unwrap().claimed);
    }

    #[tokio::test]
    async fn await_any_drains_ready_outside_wait_set_with_content() {
        let sup = JobSupervisor::new();
        let conv = "c1";
        let a = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        let b = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        sup.mark_running(&a);
        sup.mark_running(&b);
        sup.finish(&a, JobStatus::Completed, Some("done-a".into()), None);

        let parent = CancellationToken::new();
        let started = tokio::time::Instant::now();
        let result = sup
            .await_jobs(
                conv,
                JobCaller::Lead,
                Some(vec![b.clone()]),
                AwaitMode::Any,
                Some(Duration::from_secs(5)),
                &parent,
                3,
                        )
            .await
            .expect("lead await");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.jobs[0].job_id, a);
        assert_eq!(result.jobs[0].content.as_deref(), Some("done-a"));
        assert_eq!(result.running, vec![b.clone()]);
        assert!(result.unclaimed.is_empty());
        assert_eq!(result.running_count, 1);
        assert_eq!(result.idle_slots, 3);
        assert_eq!(result.pool_running, 0);
        assert!(!result.timed_out);
        let a_row = sup
            .list(conv, false, JobCaller::Lead)
            .into_iter()
            .find(|j| j.job_id == a)
            .unwrap();
        assert!(a_row.claimed);
    }

    #[tokio::test]
    async fn await_all_waits_until_every_id_is_terminal() {
        let sup = JobSupervisor::new();
        let conv = "c1";
        let a = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        let b = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
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
                JobCaller::Lead,
                Some(vec![a, b]),
                AwaitMode::All,
                Some(Duration::from_secs(2)),
                &parent,
                4,
                        )
            .await
            .expect("lead await");
        assert_eq!(result.jobs.len(), 2);
        assert!(result.running.is_empty());
        assert!(!result.timed_out);
    }

    #[tokio::test]
    async fn await_all_drains_finished_outside_wait_set() {
        let sup = JobSupervisor::new();
        let conv = "c1";
        let a = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        let b = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        sup.finish(&a, JobStatus::Completed, Some("done-a".into()), None);
        sup.finish(&b, JobStatus::Completed, Some("done-b".into()), None);
        let parent = CancellationToken::new();
        let result = sup
            .await_jobs(
                conv,
                JobCaller::Lead,
                Some(vec![b.clone()]),
                AwaitMode::All,
                Some(Duration::from_secs(1)),
                &parent,
                3,
                        )
            .await
            .expect("lead await");
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
        let a = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        let b = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        sup.mark_running(&a);
        sup.mark_running(&b);
        sup.finish(&a, JobStatus::Completed, Some("first".into()), None);

        let parent = CancellationToken::new();
        let first = sup
            .await_jobs(
                conv,
                JobCaller::Lead,
                Some(vec![a.clone(), b.clone()]),
                AwaitMode::Any,
                Some(Duration::from_secs(1)),
                &parent,
                3,
                        )
            .await
            .expect("lead await");
        assert_eq!(first.jobs.len(), 1);
        assert_eq!(first.jobs[0].job_id, a);
        assert!(sup.is_claimed(&a));

        sup.finish(&b, JobStatus::Completed, Some("second".into()), None);
        let second = sup
            .await_jobs(
                conv,
                JobCaller::Lead,
                Some(vec![a.clone(), b.clone()]),
                AwaitMode::All,
                Some(Duration::from_secs(1)),
                &parent,
                3,
                        )
            .await
            .expect("lead await");
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
        let a = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        sup.mark_running(&a);
        let parent = CancellationToken::new();
        let result = sup
            .await_jobs(
                conv,
                JobCaller::Lead,
                Some(vec![a.clone()]),
                AwaitMode::All,
                Some(Duration::from_millis(20)),
                &parent,
                4,
                        )
            .await
            .expect("lead await");
        assert!(result.timed_out);
        assert!(result.jobs.is_empty());
        assert_eq!(result.running, vec![a.clone()]);
        assert!(!sup.list(conv, false, JobCaller::Lead)[0].claimed);
        assert_eq!(sup.status(conv, &a, JobCaller::Lead).unwrap().status, "running");
    }

    #[tokio::test]
    async fn timeout_with_finished_sibling_does_not_deliver_unclaimed_content() {
        let sup = JobSupervisor::new();
        let conv = "c1";
        let done = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        let running = sup.register(conv, kind(), CancellationToken::new(), "test-run", Vec::new());
        sup.mark_running(&done);
        sup.mark_running(&running);
        sup.finish(
            &done,
            JobStatus::Completed,
            Some("secret body".into()),
            None,
        );
        let parent = CancellationToken::new();
        let result = sup
            .await_jobs(
                conv,
                JobCaller::Lead,
                Some(vec![done.clone(), running.clone()]),
                AwaitMode::All,
                Some(Duration::from_millis(20)),
                &parent,
                4,
                        )
            .await
            .expect("lead await");
        assert!(result.timed_out);
        assert!(
            result.jobs.is_empty(),
            "timeout must not put content into jobs without claiming"
        );
        assert_eq!(result.running, vec![running.clone()]);
        assert_eq!(result.unclaimed, vec![done.clone()]);
        assert!(!sup.is_claimed(&done));

        let claimed = sup
            .await_jobs(
                conv,
                JobCaller::Lead,
                Some(vec![done.clone()]),
                AwaitMode::All,
                Some(Duration::from_secs(1)),
                &parent,
                4,
                        )
            .await
            .expect("lead await");
        assert_eq!(claimed.jobs.len(), 1);
        assert_eq!(claimed.jobs[0].content.as_deref(), Some("secret body"));
        assert!(sup.is_claimed(&done));
    }

    #[tokio::test]
    async fn root_acquire_fifo_respects_cap_cancel_and_isolation() {
        let sup = Arc::new(JobSupervisor::new());
        let a = "conv-a";
        let b = "conv-b";
        let lease_a = sup
            .acquire_root(a, 1, &CancellationToken::new())
            .await
            .expect("a root");
        assert_eq!(sup.pool_running_roots(a), 1);
        assert_eq!(sup.idle_slots(a, 1), 0);

        // B is independent — not blocked by A's root.
        let lease_b = sup
            .acquire_root(b, 1, &CancellationToken::new())
            .await
            .expect("b root");
        assert_eq!(sup.pool_running_roots(b), 1);

        let cancel = CancellationToken::new();
        let wait = {
            let sup = Arc::clone(&sup);
            let cancel = cancel.clone();
            tokio::spawn(async move { sup.acquire_root(a, 1, &cancel).await })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(sup.pool_waiter_len(a), 1);
        cancel.cancel();
        assert!(wait.await.unwrap().is_none());
        assert_eq!(sup.pool_waiter_len(a), 0);

        drop(lease_a);
        assert_eq!(sup.pool_running_roots(a), 0);
        let again = sup
            .acquire_root(a, 1, &CancellationToken::new())
            .await
            .expect("a again");
        drop(again);
        drop(lease_b);
    }

    #[tokio::test]
    async fn root_fifo_transfers_slot_to_next_waiter() {
        let sup = Arc::new(JobSupervisor::new());
        let conv = "c1";
        let first = sup
            .acquire_root(conv, 1, &CancellationToken::new())
            .await
            .unwrap();
        let wait = {
            let sup = Arc::clone(&sup);
            tokio::spawn(async move { sup.acquire_root(conv, 1, &CancellationToken::new()).await })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(sup.pool_waiter_len(conv), 1);
        drop(first);
        let second = wait.await.unwrap().expect("transferred");
        assert_eq!(sup.pool_running_roots(conv), 1);
        assert_eq!(sup.pool_waiter_len(conv), 0);
        drop(second);
    }

    #[tokio::test]
    async fn nested_requires_ancestor_root() {
        let sup = JobSupervisor::new();
        assert!(sup.acquire_nested("c1").is_err());
        let _lease = sup
            .acquire_root("c1", 1, &CancellationToken::new())
            .await
            .unwrap();
        assert!(sup.acquire_nested("c1").is_ok());
        assert_eq!(sup.pool_running_roots("c1"), 1);
    }

    #[test]
    fn worker_needs_root_slot_only_for_first_level() {
        assert!(worker_needs_root_slot(0));
        assert!(worker_needs_root_slot(1));
        assert!(!worker_needs_root_slot(2));
    }

    #[test]
    fn cancel_conversation_signals_tokens() {
        let sup = JobSupervisor::new();
        let token = CancellationToken::new();
        let id = sup.register("c1", kind(), token.clone(), "test-run", Vec::new());
        assert_eq!(sup.cancel_conversation("c1"), 1);
        assert!(token.is_cancelled());
        assert_eq!(sup.cancel_token(&id).unwrap().is_cancelled(), true);
    }

    #[test]
    fn list_distinguishes_terminal_from_subagent() {
        let sup = JobSupervisor::new();
        let sub = sup.register("c1", kind(), CancellationToken::new(), "test-run", Vec::new());
        let term = sup.register(
            "c1",
            JobKind::Terminal(JobKindTerminal {
                tool_call_id: "tc-term".into(),
                message_id: "m1".into(),
                command: "cargo test".into(),
                label: Some("跑测试".into()),
            }),
            CancellationToken::new(),
            "test-run",
            Vec::new()
        );
        let listed = sup.list("c1", false, JobCaller::Lead);
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
        let id = sup.register("c1", kind(), CancellationToken::new(), "test-run", Vec::new());
        sup.finish(
            &id,
            JobStatus::Completed,
            Some("worker handoff markdown".into()),
            None,
        );
        let listed = sup.list("c1", false, JobCaller::Lead);
        assert!(listed[0].content.is_none());
        let st = sup.status("c1", &id, JobCaller::Lead).unwrap();
        assert!(st.content.is_none());
        assert_eq!(st.status, "completed");
        assert!(!st.claimed);
        let peeked = sup.list("c1", true, JobCaller::Lead);
        assert_eq!(
            peeked[0].content.as_deref(),
            Some("worker handoff markdown")
        );
    }

    #[test]
    fn occupancy_by_conversation_skips_terminal_jobs() {
        let sup = JobSupervisor::new();
        let live = sup.register("c1", kind(), CancellationToken::new(), "test-run", Vec::new());
        let done = sup.register("c1", kind(), CancellationToken::new(), "test-run", Vec::new());
        let other = sup.register("c2", kind(), CancellationToken::new(), "test-run", Vec::new());
        sup.mark_running(&live);
        sup.finish(&done, JobStatus::Completed, Some("ok".into()), None);
        let rows = sup.occupancy_by_conversation();
        assert_eq!(rows, vec![("c1".into(), 1), ("c2".into(), 1)]);
        assert_eq!(sup.running_count_for_conversation("c1"), 1);
        let _ = (live, other);
    }

    #[test]
    fn followup_reserve_rejects_a_second_claim_until_drop() {
        let sup = JobSupervisor::new();
        let first = sup
            .try_reserve_followup("c1", "inst-job")
            .expect("first claim");
        assert!(sup.subagent_instance_busy("c1", "inst-job"));
        assert!(sup.try_reserve_followup("c1", "inst-job").is_none());
        drop(first);
        assert!(!sup.subagent_instance_busy("c1", "inst-job"));
        assert!(sup.try_reserve_followup("c1", "inst-job").is_some());
    }

    #[test]
    fn followup_reserve_rejects_a_live_job() {
        let sup = JobSupervisor::new();
        let _job = sup.register("c1", kind(), CancellationToken::new(), "test-run", Vec::new());
        assert!(sup.try_reserve_followup("c1", "inst-job").is_none());
    }

    #[test]
    fn occupancy_items_include_background_terminals() {
        let sup = JobSupervisor::new();
        let sub = sup.register("c1", kind(), CancellationToken::new(), "test-run", Vec::new());
        let term = sup.register(
            "c1",
            JobKind::Terminal(JobKindTerminal {
                tool_call_id: "tc-nested".into(),
                message_id: "scoped-m".into(),
                command: "python scrape.py --city jhb".into(),
                label: Some("线 1 约堡".into()),
            }),
            CancellationToken::new(),
            "test-run",
            Vec::new()
        );
        sup.mark_running(&sub);
        sup.mark_running(&term);
        let items = sup.occupancy_items_for_conversation("c1");
        assert_eq!(items.len(), 2);
        let term_row = items
            .iter()
            .find(|j| j.kind == "terminal")
            .expect("terminal");
        assert_eq!(term_row.title.as_deref(), Some("线 1 约堡"));
        assert_eq!(term_row.job_id, term);
        let ev = sup.background_jobs_event("c1");
        match ev {
            crate::models::StreamEvent::BackgroundJobs {
                running_count,
                jobs,
                ..
            } => {
                assert_eq!(running_count, 2);
                assert_eq!(jobs.len(), 2);
            }
            other => panic!("unexpected {other:?}"),
        }
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
            "test-run",
            Vec::new()
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

    #[test]
    fn claim_pushable_skips_cancelled_and_already_claimed() {
        let sup = JobSupervisor::new();
        let done = sup.register("c1", kind(), CancellationToken::new(), "test-run", Vec::new());
        let failed = sup.register("c1", kind(), CancellationToken::new(), "test-run", Vec::new());
        let cancelled = sup.register("c1", kind(), CancellationToken::new(), "test-run", Vec::new());
        let awaited = sup.register("c1", kind(), CancellationToken::new(), "test-run", Vec::new());
        sup.finish(&done, JobStatus::Completed, Some("ok".into()), None);
        sup.finish(&failed, JobStatus::Failed, None, Some("boom".into()));
        sup.finish(
            &cancelled,
            JobStatus::Cancelled,
            None,
            Some("cancelled".into()),
        );
        sup.finish(&awaited, JobStatus::Completed, Some("secret".into()), None);
        assert!(sup.claim_if_unclaimed(&awaited).is_some());

        let items = sup.claim_pushable("c1");
        let ids: Vec<_> = items.iter().map(|i| i.job_id.as_str()).collect();
        assert!(ids.contains(&done.as_str()));
        assert!(ids.contains(&failed.as_str()));
        assert!(!ids.contains(&cancelled.as_str()));
        assert!(!ids.contains(&awaited.as_str()));
        assert!(sup.is_claimed(&done));
        assert!(sup.claim_pushable("c1").is_empty());

        sup.unclaim(&[done.clone()]);
        assert!(!sup.is_claimed(&done));
        let again = sup.claim_pushable("c1");
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].job_id, done);
    }

    #[test]
    fn claim_pushable_skips_jobs_spawned_by_subagent() {
        let sup = JobSupervisor::new();
        let lead = sup.register("c1", kind(), CancellationToken::new(), "test-run", Vec::new());
        let nested = sup.register(
            "c1",
            kind(),
            CancellationToken::new(),
            "test-run",
            vec!["inst-a".into()],
        );
        sup.finish(&lead, JobStatus::Completed, Some("lead-done".into()), None);
        sup.finish(
            &nested,
            JobStatus::Completed,
            Some("nested-done".into()),
            None,
        );
        let items = sup.claim_pushable("c1");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].job_id, lead);
        assert!(!sup.is_claimed(&nested));
        let awaited = sup.claim_if_unclaimed(&nested).unwrap();
        assert_eq!(awaited.content.as_deref(), Some("nested-done"));
    }

    #[test]
    fn open_jobs_spawned_by_lists_only_live_jobs_of_that_agent() {
        let sup = JobSupervisor::new();
        let lead = sup.register("c1", kind(), CancellationToken::new(), "test-run", Vec::new());
        let live = sup.register(
            "c1",
            kind(),
            CancellationToken::new(),
            "test-run",
            vec!["inst-a".into()],
        );
        let done = sup.register(
            "c1",
            kind(),
            CancellationToken::new(),
            "test-run",
            vec!["inst-a".into()],
        );
        sup.finish(&done, JobStatus::Completed, Some("gone".into()), None);
        let open = sup.open_jobs_spawned_by("c1", "inst-a");
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].job_id, live);
        assert_eq!(open[0].status, "queued");
        assert!(sup.open_jobs_spawned_by("c1", "inst-b").is_empty());
        assert!(sup.open_jobs_spawned_by("c1", " ").is_empty());
        assert_ne!(open[0].job_id, lead);
    }

    fn kind_for(instance_id: &str) -> JobKind {
        JobKind::Subagent(JobKindSubagent {
            tool_call_id: "tc1".into(),
            message_id: "m1".into(),
            agent_id: "explore".into(),
            title: "map auth".into(),
            agent_instance_id: instance_id.into(),
        })
    }

    fn subtree_kind(chain: &[&str]) -> JobKind {
        kind_for(chain.last().copied().unwrap_or("inst"))
    }

    /// `lead -> coder(inst-coder) -> explore(inst-explore)`.
    /// The coder's own job is lead-owned, so it is structurally invisible to the
    /// coder — an id-less `await` must return at once instead of waiting itself.
    #[tokio::test]
    async fn await_without_ids_excludes_own_parent_spawned_job() {
        let sup = JobSupervisor::new();
        let conv = "c-subtree";
        let own = sup.register(
            conv,
            subtree_kind(&["inst-coder"]),
            CancellationToken::new(),
            "run",
            Vec::new(),
        );
        sup.mark_running(&own);

        let started = tokio::time::Instant::now();
        let result = sup
            .await_jobs(
                conv,
                JobCaller::Instance("inst-coder"),
                None,
                AwaitMode::All,
                Some(Duration::from_secs(30)),
                &CancellationToken::new(),
                4,
            )
            .await
            .expect("scoped await");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a worker must never wait for its own job"
        );
        assert!(result.jobs.is_empty());
        assert!(result.running.is_empty());
        // Occupancy numbers stay conversation-wide for the UI.
        assert_eq!(result.running_count, 1);
        assert!(!sup.is_claimed(&own));
    }

    /// Same shape, but with a descendant still to finish: the coder's `await all`
    /// resolves on the descendant while its own job keeps running.
    #[tokio::test]
    async fn await_all_waits_only_for_descendants() {
        let sup = JobSupervisor::new();
        let conv = "c-subtree";
        let own = sup.register(
            conv,
            subtree_kind(&["inst-coder"]),
            CancellationToken::new(),
            "run",
            Vec::new(),
        );
        let child = sup.register(
            conv,
            subtree_kind(&["inst-coder", "inst-explore"]),
            CancellationToken::new(),
            "run",
            vec!["inst-coder".into()],
        );
        sup.mark_running(&own);
        sup.mark_running(&child);
        sup.finish(
            &child,
            JobStatus::Completed,
            Some("explore-done".into()),
            None,
        );

        let started = tokio::time::Instant::now();
        let result = sup
            .await_jobs(
                conv,
                JobCaller::Instance("inst-coder"),
                None,
                AwaitMode::All,
                Some(Duration::from_secs(30)),
                &CancellationToken::new(),
                4,
            )
            .await
            .expect("scoped await");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.jobs[0].job_id, child);
        assert_eq!(result.jobs[0].content.as_deref(), Some("explore-done"));
        assert!(result.running.is_empty());
        assert_eq!(result.running_count, 1);
    }

    #[tokio::test]
    async fn await_sees_descendant_jobs() {
        let sup = JobSupervisor::new();
        let conv = "c-subtree";
        let own = sup.register(
            conv,
            subtree_kind(&["inst-coder"]),
            CancellationToken::new(),
            "run",
            Vec::new(),
        );
        let child = sup.register(
            conv,
            subtree_kind(&["inst-coder", "inst-explore"]),
            CancellationToken::new(),
            "run",
            vec!["inst-coder".into()],
        );
        sup.mark_running(&own);
        sup.mark_running(&child);
        sup.finish(
            &child,
            JobStatus::Completed,
            Some("child-done".into()),
            None,
        );

        let result = sup
            .await_jobs(
                conv,
                JobCaller::Instance("inst-coder"),
                Some(vec![child.clone()]),
                AwaitMode::All,
                Some(Duration::from_secs(1)),
                &CancellationToken::new(),
                4,
            )
            .await
            .expect("descendant await");
        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.jobs[0].job_id, child);
        assert!(sup.is_claimed(&child));
        assert!(!sup.is_claimed(&own));
    }

    #[tokio::test]
    async fn await_rejects_foreign_job_id() {
        let sup = JobSupervisor::new();
        let conv = "c-subtree";
        let parent_job = sup.register(
            conv,
            subtree_kind(&["inst-coder"]),
            CancellationToken::new(),
            "run",
            Vec::new(),
        );
        let sibling = sup.register(
            conv,
            subtree_kind(&["inst-sibling"]),
            CancellationToken::new(),
            "run",
            vec!["inst-sibling".into()],
        );
        sup.mark_running(&parent_job);
        sup.mark_running(&sibling);

        for foreign in [parent_job.as_str(), sibling.as_str()] {
            let err = sup
                .await_jobs(
                    conv,
                    JobCaller::Instance("inst-coder"),
                    Some(vec![foreign.to_string()]),
                    AwaitMode::All,
                    Some(Duration::from_secs(1)),
                    &CancellationToken::new(),
                    4,
                )
                .await
                .expect_err("foreign jobId must be rejected");
            assert!(err.contains(foreign), "error names the id: {err}");
        }
        // The lead is unaffected: it still sees both.
        assert_eq!(sup.list(conv, false, JobCaller::Lead).len(), 2);
        assert!(sup
            .await_jobs(
                conv,
                JobCaller::Lead,
                Some(vec![parent_job.clone()]),
                AwaitMode::Any,
                Some(Duration::from_millis(50)),
                &CancellationToken::new(),
                4,
            )
            .await
            .is_ok());
    }

    #[test]
    fn list_and_status_are_scoped_to_subtree() {
        let sup = JobSupervisor::new();
        let conv = "c-subtree";
        let lead_child = sup.register(
            conv,
            subtree_kind(&["inst-other"]),
            CancellationToken::new(),
            "run",
            Vec::new(),
        );
        let own = sup.register(
            conv,
            subtree_kind(&["inst-coder"]),
            CancellationToken::new(),
            "run",
            vec!["inst-coder".into()],
        );
        let child = sup.register(
            conv,
            subtree_kind(&["inst-coder", "inst-explore"]),
            CancellationToken::new(),
            "run",
            vec!["inst-coder".into()],
        );
        let sibling = sup.register(
            conv,
            subtree_kind(&["inst-sibling"]),
            CancellationToken::new(),
            "run",
            vec!["inst-sibling".into()],
        );

        let caller = JobCaller::Instance("inst-coder");
        let ids: Vec<String> = sup
            .list(conv, false, caller)
            .into_iter()
            .map(|job| job.job_id)
            .collect();
        assert!(ids.contains(&own));
        assert!(ids.contains(&child));
        assert!(!ids.contains(&lead_child));
        assert!(!ids.contains(&sibling));

        assert!(sup.status(conv, &child, caller).is_ok());
        assert!(sup.status(conv, &lead_child, caller).is_err());
        assert!(sup.status(conv, &sibling, caller).is_err());

        // Lead regression: unchanged, conversation-wide.
        assert_eq!(sup.list(conv, false, JobCaller::Lead).len(), 4);
        assert!(sup.status(conv, &lead_child, JobCaller::Lead).is_ok());
    }

    #[test]
    fn cancel_without_ids_scoped_to_subtree() {
        let sup = JobSupervisor::new();
        let conv = "c-subtree";
        let other_token = CancellationToken::new();
        let own_token = CancellationToken::new();
        let child_token = CancellationToken::new();
        let sibling_token = CancellationToken::new();
        let other = sup.register(
            conv,
            subtree_kind(&["inst-other"]),
            other_token.clone(),
            "run",
            Vec::new(),
        );
        sup.register(
            conv,
            subtree_kind(&["inst-coder"]),
            own_token.clone(),
            "run",
            vec!["inst-coder".into()],
        );
        sup.register(
            conv,
            subtree_kind(&["inst-coder", "inst-explore"]),
            child_token.clone(),
            "run",
            vec!["inst-coder".into()],
        );
        let sibling = sup.register(
            conv,
            subtree_kind(&["inst-sibling"]),
            sibling_token.clone(),
            "run",
            vec!["inst-sibling".into()],
        );

        let outcome = sup.cancel_ids(conv, None, JobCaller::Instance("inst-coder"));
        assert_eq!(outcome.cancelled.len(), 2);
        assert!(outcome.denied.is_empty());
        assert!(own_token.is_cancelled());
        assert!(child_token.is_cancelled());
        assert!(!other_token.is_cancelled());
        assert!(!sibling_token.is_cancelled());

        // Explicit foreign ids: skipped and reported, never cancelled.
        let denied = sup.cancel_ids(
            conv,
            Some(&[other.clone(), sibling.clone()]),
            JobCaller::Instance("inst-coder"),
        );
        assert!(denied.cancelled.is_empty());
        assert_eq!(denied.denied.len(), 2);
        assert!(!other_token.is_cancelled());
        assert!(!sibling_token.is_cancelled());
    }

    #[tokio::test]
    async fn lead_await_still_sees_every_job() {
        let sup = JobSupervisor::new();
        let conv = "c-lead";
        let running = sup.register(
            conv,
            subtree_kind(&["inst-a"]),
            CancellationToken::new(),
            "run",
            Vec::new(),
        );
        let nested = sup.register(
            conv,
            subtree_kind(&["inst-a", "inst-b"]),
            CancellationToken::new(),
            "run",
            vec!["inst-a".into()],
        );
        let deep = sup.register(
            conv,
            subtree_kind(&["inst-a", "inst-b", "inst-c"]),
            CancellationToken::new(),
            "run",
            vec!["inst-a".into(), "inst-b".into()],
        );
        sup.mark_running(&running);
        sup.mark_running(&nested);
        sup.mark_running(&deep);
        sup.finish(&nested, JobStatus::Completed, Some("nested".into()), None);
        sup.finish(&deep, JobStatus::Completed, Some("deep".into()), None);

        let result = sup
            .await_jobs(
                conv,
                JobCaller::Lead,
                None,
                AwaitMode::Any,
                Some(Duration::from_secs(1)),
                &CancellationToken::new(),
                4,
            )
            .await
            .expect("lead await");
        let ids: Vec<&str> = result.jobs.iter().map(|j| j.job_id.as_str()).collect();
        assert!(ids.contains(&nested.as_str()));
        assert!(ids.contains(&deep.as_str()));
        assert_eq!(result.running, vec![running.clone()]);
        assert_eq!(sup.list(conv, false, JobCaller::Lead).len(), 3);
    }

    #[test]
    fn owner_chain_derives_direct_owner_and_lead_ownership() {
        let sup = JobSupervisor::new();
        let conv = "c-chain";
        let lead_owned = sup.register(
            conv,
            subtree_kind(&["inst-lead-child"]),
            CancellationToken::new(),
            "run",
            Vec::new(),
        );
        let direct = sup.register(
            conv,
            subtree_kind(&["inst-a"]),
            CancellationToken::new(),
            "run",
            vec!["inst-a".into()],
        );
        let grand = sup.register(
            conv,
            subtree_kind(&["inst-a", "inst-b"]),
            CancellationToken::new(),
            "run",
            vec!["inst-a".into(), "inst-b".into()],
        );

        // `open_jobs_spawned_by` keeps its meaning: direct children only.
        let open: Vec<String> = sup
            .open_jobs_spawned_by(conv, "inst-a")
            .into_iter()
            .map(|job| job.job_id)
            .collect();
        assert!(open.contains(&direct));
        assert!(!open.contains(&grand));
        let grand_open: Vec<String> = sup
            .open_jobs_spawned_by(conv, "inst-b")
            .into_iter()
            .map(|job| job.job_id)
            .collect();
        assert_eq!(grand_open, vec![grand.clone()]);
        assert!(sup.open_jobs_spawned_by(conv, "inst-nobody").is_empty());

        // Idle push keeps its meaning: lead-owned jobs only.
        sup.finish(&lead_owned, JobStatus::Completed, Some("lead".into()), None);
        sup.finish(&direct, JobStatus::Completed, Some("direct".into()), None);
        sup.finish(&grand, JobStatus::Completed, Some("grand".into()), None);
        let pushed: Vec<String> = sup
            .claim_pushable(conv)
            .into_iter()
            .map(|item| item.job_id)
            .collect();
        assert_eq!(pushed, vec![lead_owned.clone()]);

        // A descendant job stays awaitable by any ancestor on its chain.
        assert!(sup.claim_if_unclaimed(&grand).is_some());
        assert!(sup.is_claimed(&grand));
    }

    #[test]
    fn register_normalizes_blank_chain_entries() {
        let sup = JobSupervisor::new();
        let id = sup.register(
            "c1",
            kind(),
            CancellationToken::new(),
            "run",
            vec!["  ".into(), "inst-a".into()],
        );
        // Fully blank chains degrade to lead-owned (visible everywhere, idle-pushed).
        let lead = sup.register(
            "c1",
            kind(),
            CancellationToken::new(),
            "run",
            vec!["".into()],
        );
        let inner = sup.inner.lock();
        assert_eq!(inner.jobs.get(&id).unwrap().owner_chain, vec!["inst-a"]);
        assert!(inner.jobs.get(&lead).unwrap().is_lead_owned());
    }

    #[test]
    fn fanout_limit_rejects_extra_spawn() {
        let sup = JobSupervisor::new();
        let conv = "c-fanout";
        let max = 2;

        // Foreground joins hold a slot until the guard drops.
        let first = sup
            .try_begin_child_spawn(conv, Some("inst-coder"), max)
            .expect("first child");
        let second = sup
            .try_begin_child_spawn(conv, Some("inst-coder"), max)
            .expect("second child");
        let err = sup
            .try_begin_child_spawn(conv, Some("inst-coder"), max)
            .expect_err("over limit must fail");
        assert!(
            err.contains("maxChildrenPerAgent=2"),
            "error names the limit: {err}"
        );
        assert_eq!(sup.live_children_count(conv, Some("inst-coder")), 2);

        // Other instances and the lead keep their own budget.
        assert!(sup
            .try_begin_child_spawn(conv, Some("inst-other"), max)
            .is_ok());
        assert!(sup.try_begin_child_spawn(conv, None, max).is_ok());

        // Releasing one foreground join frees exactly one slot.
        drop(first);
        assert_eq!(sup.live_children_count(conv, Some("inst-coder")), 1);
        let third = sup
            .try_begin_child_spawn(conv, Some("inst-coder"), max)
            .expect("freed slot");
        drop(third);
        drop(second);
        assert_eq!(sup.live_children_count(conv, Some("inst-coder")), 0);

        // Background children are counted from the job table instead.
        let job = sup.register(
            conv,
            subtree_kind(&["inst-coder"]),
            CancellationToken::new(),
            "run",
            vec!["inst-coder".into()],
        );
        sup.mark_running(&job);
        assert_eq!(sup.live_children_count(conv, Some("inst-coder")), 1);
        assert!(sup.check_child_spawn(conv, Some("inst-coder"), 1).is_err());
        sup.finish(&job, JobStatus::Completed, Some("done".into()), None);
        assert_eq!(sup.live_children_count(conv, Some("inst-coder")), 0);
        assert!(sup.check_child_spawn(conv, Some("inst-coder"), 1).is_ok());
    }

    #[test]
    fn cancel_cascades_to_subtree() {
        let sup = JobSupervisor::new();
        let conv = "c-cascade";
        let parent_token = CancellationToken::new();
        let child_token = CancellationToken::new();
        let grand_token = CancellationToken::new();
        let sibling_token = CancellationToken::new();
        let parent = sup.register(
            conv,
            subtree_kind(&["inst-a"]),
            parent_token.clone(),
            "run",
            Vec::new(),
        );
        let child = sup.register(
            conv,
            subtree_kind(&["inst-a", "inst-b"]),
            child_token.clone(),
            "run",
            vec!["inst-a".into()],
        );
        let grand = sup.register(
            conv,
            subtree_kind(&["inst-a", "inst-b", "inst-c"]),
            grand_token.clone(),
            "run",
            vec!["inst-a".into(), "inst-b".into()],
        );
        let sibling = sup.register(
            conv,
            subtree_kind(&["inst-sibling"]),
            sibling_token.clone(),
            "run",
            Vec::new(),
        );

        // Cancelling one job explicitly takes its whole subtree with it.
        let outcome = sup.cancel_ids(conv, Some(&[parent.clone()]), JobCaller::Lead);
        assert_eq!(outcome.cancelled.len(), 3);
        assert!(outcome.cancelled.contains(&parent));
        assert!(outcome.cancelled.contains(&child));
        assert!(outcome.cancelled.contains(&grand));
        assert!(outcome.denied.is_empty());
        assert!(parent_token.is_cancelled());
        assert!(child_token.is_cancelled());
        assert!(grand_token.is_cancelled());
        assert!(!sibling_token.is_cancelled());

        // Lead `cancel` without ids still cancels the whole conversation.
        let outcome = sup.cancel_ids(conv, None, JobCaller::Lead);
        assert!(outcome.cancelled.contains(&sibling));
        assert!(sibling_token.is_cancelled());
    }

    #[tokio::test]
    async fn nested_worker_without_ancestor_root_is_promoted_to_root() {
        let sup = JobSupervisor::new();
        let conv = "c-promote";
        let parent = sup
            .acquire_root(conv, 4, &CancellationToken::new())
            .await
            .expect("ancestor root");
        assert_eq!(sup.pool_running_roots(conv), 1);

        let nested = sup
            .acquire_worker_slot(conv, 4, &CancellationToken::new(), false)
            .await
            .expect("nested lease under an ancestor root");
        assert!(matches!(nested, WorkerSlotLease::Nested(_)));
        assert_eq!(sup.pool_running_roots(conv), 1);
        drop(nested);

        parent.release();
        assert_eq!(sup.pool_running_roots(conv), 0);

        // Ancestor root gone: a nested worker is promoted, never left unaccounted.
        let promoted = sup
            .acquire_worker_slot(conv, 4, &CancellationToken::new(), false)
            .await
            .expect("promoted root lease");
        assert!(matches!(promoted, WorkerSlotLease::Root(_)));
        assert_eq!(sup.pool_running_roots(conv), 1);

        // The pool cap still applies: a full pool makes the promotion wait.
        let waited = tokio::time::timeout(
            Duration::from_millis(80),
            sup.acquire_worker_slot(conv, 1, &CancellationToken::new(), true),
        )
        .await;
        assert!(waited.is_err(), "cap=1 with a root held must wait, not exceed");
        assert_eq!(sup.pool_running_roots(conv), 1);
    }
}
