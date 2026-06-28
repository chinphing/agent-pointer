//! Per-conversation lane queue with a global concurrency cap.
//!
//! Design borrows from openclaw `enqueueCommandInLane` (per-session
//! serialization) plus a global semaphore (`agents.defaults.maxConcurrent`).
//! Runs in the same lane run strictly in arrival order; across lanes, up to
//! `max_concurrent` runs execute at once and the rest wait.
//!
//! Permit handoff: a waiter acquires a global permit *before* enqueueing, then
//! parks until its lane slot frees. When the running run drops its
//! [`LaneGuard`], the guard pops the next waiter for that lane and hands back
//! the waiter's stored global permit together with a fresh lane guard. This
//! keeps the global permit travelling with the waiter (no extra acquire) while
//! guaranteeing one run per lane.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use super::trigger::TriggerRequest;

/// A queued run waiting for its lane slot. The waiter already holds a global
/// permit (stored here) so the waker can hand it back without re-acquiring.
struct QueuedRun {
    #[allow(dead_code)]
    req: TriggerRequest,
    cancel: CancellationToken,
    global_permit: Option<OwnedSemaphorePermit>,
    waker: tokio::sync::oneshot::Sender<Permit>,
}

/// Permit handed to a run when it leaves the queue. Holds both the lane slot
/// (released on drop via [`LaneGuard`]) and the global semaphore permit.
pub struct Permit {
    _global: OwnedSemaphorePermit,
    /// Held for RAII drop; releases the lane slot and wakes the next waiter.
    #[allow(dead_code)]
    lane_guard: LaneGuard,
}

/// Releases the lane slot when dropped (RAII) and wakes the next waiter for
/// the same lane, handing back the waiter's stored global permit.
struct LaneGuard {
    lane: String,
    state: Arc<QueueState>,
}

impl Drop for LaneGuard {
    fn drop(&mut self) {
        let mut lanes = self.state.lanes.lock();
        // We are the active run for this lane; pop the next waiter.
        let next: Option<QueuedRun> = lanes
            .waiters
            .get_mut(&self.lane)
            .and_then(|q| q.pop_front());
        match next {
            // Hand the lane slot + the waiter's stored global permit to the
            // next run. The lane stays "active" (now owned by the woken run).
            Some(mut next) => {
                let global = next.global_permit.take();
                // Re-insert lane as active on behalf of the woken run.
                lanes.active.insert(self.lane.clone(), ());
                drop(lanes);
                let permit = match global {
                    Some(g) => Permit {
                        _global: g,
                        lane_guard: LaneGuard {
                            lane: self.lane.clone(),
                            state: self.state.clone(),
                        },
                    },
                    None => {
                        // Should never happen: waiters always store a permit.
                        // If it does, the waiter will see a closed channel and
                        // return `QueueError::Closed`; it can retry.
                        log::error!(
                            "run_queue: waiter for lane {} had no global permit",
                            self.lane
                        );
                        return;
                    }
                };
                if next.waker.send(permit).is_err() {
                    // Waiter was cancelled / dropped. Roll back the lane slot
                    // we just took on its behalf so the next waiter (if any)
                    // can be served by the next Drop.
                    let mut lanes = self.state.lanes.lock();
                    lanes.active.remove(&self.lane);
                    if let Some(q) = lanes.waiters.get_mut(&self.lane) {
                        if let Some(mut retry) = q.pop_front() {
                            let g = retry.global_permit.take();
                            lanes.active.insert(self.lane.clone(), ());
                            drop(lanes);
                            if let Some(g) = g {
                                let _ = retry.waker.send(Permit {
                                    _global: g,
                                    lane_guard: LaneGuard {
                                        lane: self.lane.clone(),
                                        state: self.state.clone(),
                                    },
                                });
                            }
                        }
                    }
                }
            }
            // No waiter: just release the lane slot.
            None => {
                lanes.active.remove(&self.lane);
                if lanes.waiters.get(&self.lane).map_or(false, |q| q.is_empty()) {
                    lanes.waiters.remove(&self.lane);
                }
            }
        }
    }
}

struct QueueState {
    lanes: Mutex<LanesState>,
    global: Arc<Semaphore>,
    max_concurrent: usize,
}

struct LanesState {
    active: HashMap<String, ()>,
    waiters: HashMap<String, VecDeque<QueuedRun>>,
}

/// In-process run queue. Owned by [`super::RunDispatcher`]. Cheap to clone
/// (one `Arc`); clones share the same underlying lanes/semaphore.
#[derive(Clone)]
pub struct RunQueue {
    state: Arc<QueueState>,
}

impl RunQueue {
    pub fn new(max_concurrent: usize) -> Self {
        let max_concurrent = max_concurrent.max(1);
        Self {
            state: Arc::new(QueueState {
                lanes: Mutex::new(LanesState {
                    active: HashMap::new(),
                    waiters: HashMap::new(),
                }),
                global: Arc::new(Semaphore::new(max_concurrent)),
                max_concurrent,
            }),
        }
    }

    pub fn max_concurrent(&self) -> usize {
        self.state.max_concurrent
    }

    /// Enqueue a run. Resolves once a lane slot + global permit are available.
    /// Cancelling `cancel` drops the run from the queue (if still waiting)
    /// and resolves with [`QueueError::Cancelled`].
    ///
    /// The returned [`Permit`] must be held for the lifetime of the run;
    /// dropping it releases both the lane slot and the global permit (and
    /// wakes the next waiter for the lane).
    pub async fn acquire(
        &self,
        req: TriggerRequest,
        cancel: CancellationToken,
    ) -> Result<Permit, QueueError> {
        let lane = req
            .lane
            .clone()
            .or_else(|| req.conversation_id.clone())
            .unwrap_or_else(|| "default".to_string());

        // Acquire global permit first (fair across lanes). Cancellable.
        let global = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(QueueError::Cancelled),
            res = self.state.global.clone().acquire_owned() => {
                res.map_err(|_| QueueError::Closed)?
            }
        };

        // Slow path channel must outlive the locking block below so the
        // select! at the end can consume `rx`.
        let (tx, rx) = tokio::sync::oneshot::channel();

        // Fast path: lane free.
        {
            let mut lanes = self.state.lanes.lock();
            if !lanes.active.contains_key(&lane) {
                lanes.active.insert(lane.clone(), ());
                return Ok(Permit {
                    _global: global,
                    lane_guard: LaneGuard {
                        lane,
                        state: self.state.clone(),
                    },
                });
            }
            // Slow path: park as a waiter (keeping our global permit).
            lanes.waiters.entry(lane.clone()).or_default().push_back(QueuedRun {
                req: TriggerRequest { lane: Some(lane.clone()), ..req },
                cancel: cancel.clone(),
                global_permit: Some(global),
                waker: tx,
            });
            drop(lanes);
        }

        // Wait for the lane guard to wake us, or bail on cancellation.
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                self.remove_waiter(&lane);
                // We are still parked; the channel send will fail (receiver
                // dropped) and the lane guard will roll back. Return Cancelled.
                Err(QueueError::Cancelled)
            }
            res = rx => match res {
                Ok(permit) => Ok(permit),
                Err(_) => Err(QueueError::Closed),
            },
        }
    }

    fn remove_waiter(&self, lane: &str) {
        let mut lanes = self.state.lanes.lock();
        if let Some(q) = lanes.waiters.get_mut(lane) {
            q.retain(|r| !r.cancel.is_cancelled());
            if q.is_empty() {
                lanes.waiters.remove(lane);
            }
        }
    }

    /// Number of runs currently executing.
    pub fn active_count(&self) -> usize {
        self.state.lanes.lock().active.len()
    }

    /// Number of runs waiting across all lanes.
    pub fn waiting_count(&self) -> usize {
        self.state
            .lanes
            .lock()
            .waiters
            .values()
            .map(|q| q.len())
            .sum()
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

    fn req(lane: &str) -> TriggerRequest {
        TriggerRequest {
            run_id: None,
            idempotency_key: None,
            conversation_id: Some(lane.to_string()),
            trigger_source: TriggerSource::Ipc,
            trigger_meta: TriggerMeta::empty(),
            lane: Some(lane.to_string()),
            messages: Vec::<ChatMessage>::new(),
            enabled_skill_ids: vec![],
            agent_mode: None,
            lead_agent_id: None,
            tool_rounds_used_single_start: 0,
            tool_rounds_used_supervisor_start: 0,
            workspace_root: String::new(),
            workspace_inherit_disabled: None,
            deliver: crate::dispatcher::trigger::DeliverTarget::None,
        }
    }

    #[tokio::test]
    async fn same_lane_serializes() {
        let q = RunQueue::new(4);
        let c1 = CancellationToken::new();
        let c2 = CancellationToken::new();
        let p1 = q.acquire(req("L"), c1.clone()).await.unwrap();
        // L is busy: second acquire should park.
        let q2 = q.clone();
        let c2c = c2.clone();
        let h = tokio::spawn(async move { q2.acquire(req("L"), c2c).await });
        // Give it a moment to park.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(q.waiting_count(), 1);
        // Release p1 -> h should resolve.
        drop(p1);
        let p2 = h.await.unwrap().unwrap();
        assert_eq!(q.active_count(), 1);
        drop(p2);
    }

    #[tokio::test]
    async fn global_cap_limits_concurrency() {
        let q = RunQueue::new(1);
        let c1 = CancellationToken::new();
        let c2 = CancellationToken::new();
        let p1 = q.acquire(req("A"), c1).await.unwrap();
        // Global cap=1, different lane: second acquire parks on the global
        // semaphore (not a lane waiter), so waiting_count() stays 0 but the
        // acquire must not complete.
        let q2 = q.clone();
        let h = tokio::spawn(async move { q2.acquire(req("B"), c2).await });
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert!(
            !h.is_finished(),
            "second acquire completed under global cap=1"
        );
        assert_eq!(q.active_count(), 1);
        // Release p1 -> h should now resolve.
        drop(p1);
        let _ = h.await.unwrap().unwrap();
    }
}
