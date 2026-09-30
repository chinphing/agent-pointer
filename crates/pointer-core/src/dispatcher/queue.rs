//! Per-lane FIFO queues with nested session + global gates (openclaw-style).
//!
//! Each run acquires two lane slots in order:
//! 1. `session:{conversation}` — serializes turns in one conversation (`max=1`).
//! 2. `global:main` or `global:cron` — caps simultaneous runs of that class
//!    (`max = maxConcurrentRuns` from settings).
//!
//! Unlike the old global `Semaphore`, waiters park only on their lane queue and
//! do not hold global slots while waiting for a session slot (or vice versa).

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use serde::{Deserialize, Serialize};

use super::trigger::{TriggerRequest, TriggerSource};
use crate::models::BackgroundJobView;

/// A run waiting in a lane FIFO queue (observability / settings UI).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueWaiterView {
    pub run_id: String,
    pub conversation_id: String,
    pub trigger_source: String,
}

/// Per-lane queue depth and waiters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaneQueueView {
    pub lane: String,
    pub active: usize,
    pub waiting: usize,
    pub max_concurrent: usize,
    pub waiters: Vec<QueueWaiterView>,
}

/// In-memory lane queue snapshot (excludes DB-only queued runs).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueLanesSnapshot {
    pub max_concurrent_main: usize,
    pub max_concurrent_cron: usize,
    pub lanes: Vec<LaneQueueView>,
}

/// Persisted run still in `queued` status (waiting for a lane slot).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingRunView {
    pub run_id: String,
    pub conversation_id: String,
    pub trigger_source: String,
    pub created_at_ms: i64,
}

/// Background job occupancy for one conversation (not a dispatcher lane).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundJobOccupancyView {
    pub conversation_id: String,
    pub running_count: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jobs: Vec<BackgroundJobView>,
}

/// Combined dispatcher queue view for settings / ops UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunQueueSnapshot {
    pub max_concurrent_main: usize,
    pub max_concurrent_cron: usize,
    pub lanes: Vec<LaneQueueView>,
    pub pending_runs: Vec<PendingRunView>,
    /// Jobs still queued/running. Empty means occupancy should be 0.
    /// Independent of `lanes` / `pendingRuns` (parent turn vs background jobs).
    #[serde(default)]
    pub background_jobs: Vec<BackgroundJobOccupancyView>,
}

impl From<&QueueWaitMeta> for QueueWaiterView {
    fn from(m: &QueueWaitMeta) -> Self {
        Self {
            run_id: m.run_id.clone(),
            conversation_id: m.conversation_id.clone(),
            trigger_source: m.trigger_source.as_str().to_string(),
        }
    }
}
pub const LANE_MAIN: &str = "global:main";
/// Isolated pool for scheduled cron dispatches.
pub const LANE_CRON: &str = "global:cron";

/// Map a conversation / explicit lane key to the session lane id.
pub fn resolve_session_lane(key: &str) -> String {
    let cleaned = key.trim();
    if cleaned.is_empty() {
        return "session:default".to_string();
    }
    if cleaned.starts_with("session:") {
        cleaned.to_string()
    } else {
        format!("session:{cleaned}")
    }
}

/// Global resource lane for a trigger source.
pub fn resolve_global_lane(source: TriggerSource) -> &'static str {
    match source {
        TriggerSource::Cron => LANE_CRON,
        _ => LANE_MAIN,
    }
}

struct QueuedEntry {
    cancel: CancellationToken,
    waker: tokio::sync::oneshot::Sender<()>,
    meta: QueueWaitMeta,
}

#[derive(Clone, Debug)]
struct QueueWaitMeta {
    run_id: String,
    conversation_id: String,
    trigger_source: TriggerSource,
}

struct LaneState {
    max_concurrent: usize,
    active: usize,
    queue: VecDeque<QueuedEntry>,
}

impl LaneState {
    fn new(max_concurrent: usize) -> Self {
        Self {
            max_concurrent: max_concurrent.max(1),
            active: 0,
            queue: VecDeque::new(),
        }
    }
}

struct LaneRegistryInner {
    lanes: Mutex<HashMap<String, LaneState>>,
    main_max: Mutex<usize>,
    cron_max: Mutex<usize>,
}

/// RAII slot in one lane; drop decrements active and pumps the lane queue.
struct LaneSlotGuard {
    lane: String,
    registry: Arc<LaneRegistryInner>,
}

impl Drop for LaneSlotGuard {
    fn drop(&mut self) {
        self.registry.release_and_pump(&self.lane);
    }
}

/// Holds session + global lane slots for the lifetime of a run.
pub struct Permit {
    _session: LaneSlotGuard,
    _global: LaneSlotGuard,
}

/// In-process run queue. Owned by [`super::RunDispatcher`]. Cheap to clone.
#[derive(Clone)]
pub struct RunQueue {
    inner: Arc<LaneRegistryInner>,
}

impl RunQueue {
    pub fn new(max_concurrent: usize) -> Self {
        let max = max_concurrent.max(1);
        let inner = Arc::new(LaneRegistryInner {
            lanes: Mutex::new(HashMap::new()),
            main_max: Mutex::new(max),
            cron_max: Mutex::new(max),
        });
        inner.ensure_lane(LANE_MAIN, max);
        inner.ensure_lane(LANE_CRON, max);
        Self { inner }
    }

    pub fn max_concurrent(&self) -> usize {
        *self.inner.main_max.lock()
    }

    /// Update `global:main` and `global:cron` concurrency caps and pump waiters.
    pub fn set_max_concurrent(&self, new_max: usize) {
        let new_max = new_max.max(1);
        {
            let mut main = self.inner.main_max.lock();
            if *main == new_max {
                return;
            }
            *main = new_max;
        }
        *self.inner.cron_max.lock() = new_max;
        self.inner.set_lane_max(LANE_MAIN, new_max);
        self.inner.set_lane_max(LANE_CRON, new_max);
        log::info!("run_queue: global lane max_concurrent -> {new_max}");
    }

    /// Enqueue a run: session lane first, then global lane (openclaw nesting).
    pub async fn acquire(
        &self,
        req: TriggerRequest,
        cancel: CancellationToken,
    ) -> Result<Permit, QueueError> {
        let session_key = req
            .lane
            .as_deref()
            .or(req.conversation_id.as_deref())
            .unwrap_or("default");
        let session_lane = resolve_session_lane(session_key);
        let global_lane = resolve_global_lane(req.trigger_source);
        let meta = QueueWaitMeta {
            run_id: req
                .run_id
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_default(),
            conversation_id: req
                .conversation_id
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| session_key.to_string()),
            trigger_source: req.trigger_source,
        };

        let session = self
            .inner
            .acquire_lane(&session_lane, cancel.clone(), meta.clone())
            .await?;
        let global = match self.inner.acquire_lane(global_lane, cancel, meta).await {
            Ok(g) => g,
            Err(e) => {
                // Release session slot if global gate fails.
                drop(session);
                return Err(e);
            }
        };

        Ok(Permit {
            _session: session,
            _global: global,
        })
    }

    /// Runs currently holding any lane slot (session + global counted separately).
    pub fn active_count(&self) -> usize {
        self.inner.lanes.lock().values().map(|s| s.active).sum()
    }

    /// Runs waiting on any lane queue.
    pub fn waiting_count(&self) -> usize {
        self.inner
            .lanes
            .lock()
            .values()
            .map(|s| s.queue.len())
            .sum()
    }

    /// Active runs in `global:main` (for observability).
    pub fn main_active_count(&self) -> usize {
        self.inner.lane_active(LANE_MAIN)
    }

    /// Queued runs in `global:main`.
    pub fn main_waiting_count(&self) -> usize {
        self.inner.lane_waiting(LANE_MAIN)
    }

    /// True when this conversation's session lane has an active run or waiter.
    pub fn session_has_activity(&self, conversation_id: &str) -> bool {
        let lane = resolve_session_lane(conversation_id);
        let lanes = self.inner.lanes.lock();
        lanes
            .get(&lane)
            .is_some_and(|s| s.active > 0 || s.queue.iter().any(|e| !e.cancel.is_cancelled()))
    }

    /// Active runs in `global:cron`.
    pub fn cron_active_count(&self) -> usize {
        self.inner.lane_active(LANE_CRON)
    }

    /// Queued runs in `global:cron`.
    pub fn cron_waiting_count(&self) -> usize {
        self.inner.lane_waiting(LANE_CRON)
    }

    /// Lane-level queue snapshot for settings / observability UI.
    pub fn snapshot(&self) -> QueueLanesSnapshot {
        self.inner.snapshot()
    }
}

impl LaneRegistryInner {
    fn max_for_lane(&self, lane: &str) -> usize {
        if lane.starts_with("session:") {
            return 1;
        }
        match lane {
            LANE_MAIN => *self.main_max.lock(),
            LANE_CRON => *self.cron_max.lock(),
            _ => 1,
        }
    }

    fn ensure_lane(&self, lane: &str, max_concurrent: usize) {
        let mut lanes = self.lanes.lock();
        lanes
            .entry(lane.to_string())
            .or_insert_with(|| LaneState::new(max_concurrent));
    }

    fn set_lane_max(&self, lane: &str, max_concurrent: usize) {
        let max_concurrent = max_concurrent.max(1);
        let mut lanes = self.lanes.lock();
        let state = lanes
            .entry(lane.to_string())
            .or_insert_with(|| LaneState::new(max_concurrent));
        state.max_concurrent = max_concurrent;
        drop(lanes);
        self.pump_lane(lane);
    }

    fn lane_active(&self, lane: &str) -> usize {
        self.lanes.lock().get(lane).map(|s| s.active).unwrap_or(0)
    }

    fn lane_waiting(&self, lane: &str) -> usize {
        self.lanes
            .lock()
            .get(lane)
            .map(|s| s.queue.len())
            .unwrap_or(0)
    }

    fn snapshot(&self) -> QueueLanesSnapshot {
        let main_max = *self.main_max.lock();
        let cron_max = *self.cron_max.lock();
        let lanes = self.lanes.lock();

        let mut views = vec![
            Self::lane_view(&lanes, LANE_MAIN, main_max),
            Self::lane_view(&lanes, LANE_CRON, cron_max),
        ];

        let mut session_keys: Vec<String> = lanes
            .keys()
            .filter(|k| k.starts_with("session:"))
            .filter(|k| {
                let s = &lanes[*k];
                s.active > 0 || s.queue.iter().any(|e| !e.cancel.is_cancelled())
            })
            .cloned()
            .collect();
        session_keys.sort();
        for lane in session_keys {
            views.push(Self::lane_view(&lanes, &lane, 1));
        }

        QueueLanesSnapshot {
            max_concurrent_main: main_max,
            max_concurrent_cron: cron_max,
            lanes: views,
        }
    }

    fn lane_view(
        lanes: &HashMap<String, LaneState>,
        lane: &str,
        max_concurrent: usize,
    ) -> LaneQueueView {
        let Some(state) = lanes.get(lane) else {
            return LaneQueueView {
                lane: lane.to_string(),
                active: 0,
                waiting: 0,
                max_concurrent,
                waiters: Vec::new(),
            };
        };
        let waiters: Vec<QueueWaiterView> = state
            .queue
            .iter()
            .filter(|e| !e.cancel.is_cancelled())
            .map(|e| QueueWaiterView::from(&e.meta))
            .collect();
        LaneQueueView {
            lane: lane.to_string(),
            active: state.active,
            waiting: waiters.len(),
            max_concurrent,
            waiters,
        }
    }

    async fn acquire_lane(
        self: &Arc<Self>,
        lane: &str,
        cancel: CancellationToken,
        meta: QueueWaitMeta,
    ) -> Result<LaneSlotGuard, QueueError> {
        if cancel.is_cancelled() {
            return Err(QueueError::Cancelled);
        }

        let (tx, rx) = tokio::sync::oneshot::channel();
        let need_wait = {
            let mut lanes = self.lanes.lock();
            let max = self.max_for_lane(lane);
            let state = lanes
                .entry(lane.to_string())
                .or_insert_with(|| LaneState::new(max));
            state.max_concurrent = max;
            if state.active < state.max_concurrent {
                state.active += 1;
                false
            } else {
                state.queue.push_back(QueuedEntry {
                    cancel: cancel.clone(),
                    waker: tx,
                    meta,
                });
                true
            }
        };

        if !need_wait {
            return Ok(LaneSlotGuard {
                lane: lane.to_string(),
                registry: Arc::clone(self),
            });
        }

        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                self.purge_cancelled_waiters(lane);
                return Err(QueueError::Cancelled);
            }
            res = rx => match res {
                Ok(()) => {
                    return Ok(LaneSlotGuard {
                        lane: lane.to_string(),
                        registry: Arc::clone(self),
                    });
                }
                Err(_) => return Err(QueueError::Closed),
            },
        }
    }

    fn release_and_pump(&self, lane: &str) {
        {
            let mut lanes = self.lanes.lock();
            if let Some(state) = lanes.get_mut(lane) {
                if state.active > 0 {
                    state.active -= 1;
                }
            }
        }
        self.pump_lane(lane);
    }

    fn pump_lane(&self, lane: &str) {
        loop {
            let waker = {
                let mut lanes = self.lanes.lock();
                let Some(state) = lanes.get_mut(lane) else {
                    return;
                };
                if state.active >= state.max_concurrent || state.queue.is_empty() {
                    return;
                }
                let mut next = None;
                while let Some(entry) = state.queue.pop_front() {
                    if entry.cancel.is_cancelled() {
                        continue;
                    }
                    next = Some(entry);
                    break;
                }
                let Some(entry) = next else {
                    return;
                };
                state.active += 1;
                entry.waker
            };
            if waker.send(()).is_err() {
                let mut lanes = self.lanes.lock();
                if let Some(state) = lanes.get_mut(lane) {
                    if state.active > 0 {
                        state.active -= 1;
                    }
                }
                continue;
            }
            return;
        }
    }

    fn purge_cancelled_waiters(&self, lane: &str) {
        let mut lanes = self.lanes.lock();
        if let Some(state) = lanes.get_mut(lane) {
            state.queue.retain(|e| !e.cancel.is_cancelled());
        }
    }
}

#[derive(Debug)]
pub enum QueueError {
    Closed,
    Cancelled,
}

impl std::fmt::Display for QueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueueError::Closed => f.write_str("queue closed"),
            QueueError::Cancelled => f.write_str("run cancelled while queued"),
        }
    }
}

impl std::error::Error for QueueError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatcher::trigger::{TriggerMeta, TriggerSource};
    use crate::models::ChatMessage;

    fn req(lane: &str, source: TriggerSource) -> TriggerRequest {
        TriggerRequest {
            run_id: None,
            idempotency_key: None,
            conversation_id: Some(lane.to_string()),
            trigger_source: source,
            trigger_meta: TriggerMeta::empty(),
            lane: Some(lane.to_string()),
            messages: Vec::<ChatMessage>::new(),
            enabled_skill_ids: vec![],
            agent_skill_overrides: std::collections::HashMap::new(),
            lead_agent_id: None,
            performance_mode: None,
            tool_rounds_used_single_start: 0,
            workspace_root: String::new(),
            workspace_inherit_disabled: None,
            deliver: crate::dispatcher::trigger::DeliverTarget::None,
            web_session_auth: None,
        }
    }

    #[tokio::test]
    async fn same_session_lane_serializes() {
        let q = RunQueue::new(4);
        let c1 = CancellationToken::new();
        let c2 = CancellationToken::new();
        let p1 = q
            .acquire(req("L", TriggerSource::Ipc), c1.clone())
            .await
            .unwrap();
        let q2 = q.clone();
        let c2c = c2.clone();
        let h = tokio::spawn(async move { q2.acquire(req("L", TriggerSource::Ipc), c2c).await });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(q.waiting_count() >= 1);
        drop(p1);
        let p2 = h.await.unwrap().unwrap();
        drop(p2);
    }

    #[tokio::test]
    async fn session_has_activity_tracks_active_and_waiters() {
        let q = RunQueue::new(4);
        assert!(!q.session_has_activity("L"));
        let p1 = q
            .acquire(req("L", TriggerSource::Ipc), CancellationToken::new())
            .await
            .unwrap();
        assert!(q.session_has_activity("L"));
        assert!(!q.session_has_activity("other"));
        drop(p1);
        assert!(!q.session_has_activity("L"));
    }

    #[tokio::test]
    async fn global_main_cap_limits_cross_session_concurrency() {
        let q = RunQueue::new(1);
        let c1 = CancellationToken::new();
        let c2 = CancellationToken::new();
        let p1 = q.acquire(req("A", TriggerSource::Ipc), c1).await.unwrap();
        assert_eq!(q.main_active_count(), 1);

        let q2 = q.clone();
        let h = tokio::spawn(async move { q2.acquire(req("B", TriggerSource::Ipc), c2).await });
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert!(
            !h.is_finished(),
            "second session should wait on global:main cap=1"
        );
        assert_eq!(q.main_waiting_count(), 1);
        drop(p1);
        let _ = h.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn cron_uses_separate_global_lane() {
        let q = RunQueue::new(1);
        let p_ipc = q
            .acquire(req("chat-1", TriggerSource::Ipc), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(q.main_active_count(), 1);

        // Cron has its own global:cron pool at the same cap.
        let p_cron = q
            .acquire(
                req("cron:job1", TriggerSource::Cron),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(q.main_active_count(), 1);
        assert_eq!(q.cron_active_count(), 1);

        drop(p_ipc);
        drop(p_cron);
    }

    #[tokio::test]
    async fn set_max_concurrent_pumps_main_waiters() {
        let q = RunQueue::new(1);
        let p1 = q
            .acquire(req("A", TriggerSource::Ipc), CancellationToken::new())
            .await
            .unwrap();
        let q2 = q.clone();
        let h = tokio::spawn(async move {
            q2.acquire(req("B", TriggerSource::Ipc), CancellationToken::new())
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(!h.is_finished());

        q.set_max_concurrent(2);
        drop(p1);
        let p2 = h.await.unwrap().unwrap();
        drop(p2);
    }
}
