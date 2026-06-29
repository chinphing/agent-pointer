//! Unified run dispatcher: the single callable / event-triggered entry point
//! for running the agent.
//!
//! All trigger sources (Tauri IPC, HTTP Runs API, webhook, cron, IM channel,
//! internal event) normalize into a [`TriggerRequest`] and call
//! [`RunDispatcher::dispatch`]. The dispatcher owns:
//!
//! - a [`RunQueue`] (per-conversation lane + global concurrency cap),
//! - an [`AgentEventBus`] (typed, per-run-sequenced event stream),
//! - a per-run [`CancellationToken`] registry,
//! - the `runs` table lifecycle (via [`AppState::session_index`]).
//!
//! Execution itself still goes through [`crate::chat_service::run_chat`]; the
//! dispatcher wraps it with queueing, idempotency, status persistence, event
//! emission, and cancellation. This keeps Phase 1 a no-behavior-change wrap:
//! existing callers can keep calling `run_chat` directly while new callers go
//! through the dispatcher, and both share the same `AppState`.
//!
//! Design references:
//! - openclaw `agent` RPC: ack-then-stream-then-final, idempotency key, lane
//!   queue, `emitAgentEvent` bus.
//! - hermes `GatewayRunner._handle_message`: one dispatch pipeline behind many
//!   platform adapters; plugins register `pre_gateway_dispatch` hooks.

pub mod hooks;
pub mod queue;
pub mod trigger;

pub use hooks::{
    HookOutcome, HookRegistry, OnRunCancelledHook, OnRunFailedHook, OnRunFinishedHook,
    OnRunStartedHook, OnTriggerReceivedHook, PostToolCallContext, PostToolCallHook,
    PreDispatchContext, PreDispatchHook, PreToolCallContext, PreToolCallHook,
    RunCancelledContext, RunFailedContext, RunFinishedContext, RunStartedContext,
    TriggerReceivedContext,
};
pub use queue::{Permit, QueueError, RunQueue};
pub use trigger::{
    DeliverTarget, RunAcceptStatus, RunHandle, RunOutcome, TriggerMeta, TriggerRequest,
    TriggerSource,
};

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::agent_events::{stream_event_to_agent_event, AgentEvent, AgentEventBus};
use crate::chat_service::{run_chat, AppState};
use crate::conversation_store::runs::RunStatus;
use crate::models::{ChatMessage, StreamEvent};

/// Default global concurrency cap. Configurable in later phases via settings;
/// Phase 1 ships a constant.
const DEFAULT_MAX_CONCURRENT: usize = 4;

/// In-process run dispatcher. Constructed once per host process and shared by
/// all trigger sources. Cheap to clone (one `Arc`).
#[derive(Clone)]
pub struct RunDispatcher {
    inner: Arc<Inner>,
}

struct Inner {
    state: Arc<AppState>,
    queue: RunQueue,
    events: AgentEventBus,
    hooks: Arc<HookRegistry>,
    cancels: Mutex<HashMap<String, CancellationToken>>,
}

impl RunDispatcher {
    /// Build a dispatcher backed by the given [`AppState`]. Uses the default
    /// global concurrency cap and an empty [`HookRegistry`]; register hooks via
    /// [`Self::hooks_mut`] before serving triggers, or use
    /// [`Self::with_hooks`].
    pub fn new(state: Arc<AppState>) -> Self {
        Self::with_max_concurrent(state, DEFAULT_MAX_CONCURRENT)
    }

    /// Build with an explicit global concurrency cap (clamped to >= 1).
    pub fn with_max_concurrent(state: Arc<AppState>, max_concurrent: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                state,
                queue: RunQueue::new(max_concurrent),
                events: AgentEventBus::new(),
                hooks: Arc::new(HookRegistry::new()),
                cancels: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Build with a pre-populated [`HookRegistry`] (e.g. with built-in hooks
    /// registered by the host). Build the registry with `&mut` first, wrap in
    /// `Arc`, then pass here:
    ///
    /// ```ignore
    /// let mut hooks = HookRegistry::new();
    /// hooks.register_on_run_finished(Arc::new(MyHook));
    /// let dispatcher = RunDispatcher::with_hooks(state, Arc::new(hooks));
    /// ```
    pub fn with_hooks(state: Arc<AppState>, hooks: Arc<HookRegistry>) -> Self {
        Self::with_hooks_and_max_concurrent(state, hooks, DEFAULT_MAX_CONCURRENT)
    }

    /// Build with a pre-populated [`HookRegistry`] and an explicit global
    /// concurrency cap.
    pub fn with_hooks_and_max_concurrent(
        state: Arc<AppState>,
        hooks: Arc<HookRegistry>,
        max_concurrent: usize,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                state,
                queue: RunQueue::new(max_concurrent),
                events: AgentEventBus::new(),
                hooks,
                cancels: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn events(&self) -> &AgentEventBus {
        &self.inner.events
    }

    pub fn queue(&self) -> &RunQueue {
        &self.inner.queue
    }

    pub fn hooks(&self) -> &HookRegistry {
        &self.inner.hooks
    }

    /// Unified dispatch entry. Returns immediately with a [`RunHandle`]; the
    /// run executes asynchronously. Subscribe via [`events`] / [`wait`].
    ///
    /// [`events`]: RunDispatcher::events
    /// [`wait`]: RunDispatcher::wait
    pub async fn dispatch(&self, mut req: TriggerRequest) -> anyhow::Result<RunHandle> {
        // 0. on_trigger_received: hooks may rewrite or reject before any state
        // mutation. A rewrite replaces `req` for the rest of dispatch.
        {
            let mut ctx = TriggerReceivedContext { req: req.clone() };
            match self.inner.hooks.run_on_trigger_received(&mut ctx).await? {
                HookOutcome::Continue => {}
                HookOutcome::Rewrite(new_req) => {
                    log::info!("dispatch: on_trigger_received rewrote request");
                    req = new_req;
                }
                HookOutcome::Reject { reason } => {
                    log::info!("dispatch: rejected by on_trigger_received: {reason}");
                    anyhow::bail!("trigger rejected: {reason}");
                }
            }
        }

        // 1. Resolve conversation id (Phase 1: caller must supply one).
        let conversation_id = match req.conversation_id.clone() {
            Some(id) if !id.trim().is_empty() => id,
            other => anyhow::bail!(
                "dispatch: conversation_id is required (got {:?}); Phase 1 callers must supply one",
                other
            ),
        };

        // 2. Idempotency check: reuse existing run if the key matches.
        if let Some(key) = req.idempotency_key.as_deref() {
            if !key.trim().is_empty() {
                if let Ok(Some((existing_run_id, status))) =
                    self.inner.state.session_index.runs_find_by_idempotency_key(key)
                {
                    log::info!(
                        "dispatch: idempotency key {} reused existing run_id={} status={}",
                        key,
                        existing_run_id,
                        status
                    );
                    return Ok(RunHandle {
                        run_id: existing_run_id.clone(),
                        status: RunAcceptStatus::Reused,
                        reused_run_id: Some(existing_run_id),
                    });
                }
            }
        }

        // 3. Resolve run id.
        let run_id = req
            .run_id
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        req.run_id = Some(run_id.clone());

        // 3b. pre_dispatch: hooks may reject before queueing (no rewrite here;
        // the request is settled). Fires after idempotency + run-id resolution
        // so the hook sees the final run_id.
        let pre_lane = req
            .lane
            .clone()
            .or_else(|| req.conversation_id.clone())
            .unwrap_or_else(|| "default".to_string());
        {
            let ctx = PreDispatchContext {
                run_id: &run_id,
                conversation_id: &conversation_id,
                lane: &pre_lane,
                trigger_source: req.trigger_source,
            };
            match self.inner.hooks.run_pre_dispatch(&ctx).await? {
                HookOutcome::Continue => {}
                HookOutcome::Rewrite(_) => {}
                HookOutcome::Reject { reason } => {
                    log::info!("dispatch: rejected by pre_dispatch: {reason}");
                    anyhow::bail!("dispatch rejected: {reason}");
                }
            }
        }

        // 4. Persist queued row + emit RunQueued.
        let trigger_meta_json = serde_json::to_string(&req.trigger_meta).unwrap_or_else(|_| "{}".into());
        let inserted = self.inner.state.session_index.runs_insert_queued(
            &run_id,
            &conversation_id,
            req.trigger_source,
            &trigger_meta_json,
            req.idempotency_key.as_deref(),
        )?;
        if !inserted {
            // A row with this run_id already exists. Treat as reuse.
            log::warn!("dispatch: run_id {} already present in runs table; returning Reused", run_id);
            return Ok(RunHandle {
                run_id: run_id.clone(),
                status: RunAcceptStatus::Reused,
                reused_run_id: Some(run_id),
            });
        }
        self.inner.events.emit(AgentEvent::RunQueued {
            run_id: run_id.clone(),
            seq: 0,
            ts: 0,
        });

        // 5. Register cancellation token for this run.
        let cancel = CancellationToken::new();
        self.inner
            .cancels
            .lock()
            .insert(run_id.clone(), cancel.clone());

        // 6. Spawn the runner task.
        let this = self.clone();
        let trigger_source = req.trigger_source;
        let lane = req
            .lane
            .clone()
            .or_else(|| req.conversation_id.clone())
            .unwrap_or_else(|| "default".to_string());
        // Pin the resolved lane back onto the request so the queue and the
        // runner agree on the lane key (the queue recomputes it from req).
        req.lane = Some(lane.clone());
        let lane_log = lane.clone();
        let run_id_task = run_id.clone();
        let conv_id_task = conversation_id.clone();
        let web_session_auth = req.web_session_auth.clone();
        tokio::spawn(async move {
            crate::web_request_auth::run_with_optional_web_session(web_session_auth, || async move {
                this.run_runner(req, run_id_task, conv_id_task, cancel).await
            })
            .await;
        });

        log::info!(
            "dispatch: accepted run_id={} conversation_id={} source={} lane={}",
            run_id,
            conversation_id,
            trigger_source,
            lane_log,
        );

        Ok(RunHandle {
            run_id,
            status: RunAcceptStatus::Accepted,
            reused_run_id: None,
        })
    }

    /// Runner body: acquire queue slot, start run_chat, bridge stream events
    /// to the agent event bus, persist terminal status.
    async fn run_runner(
        self,
        req: TriggerRequest,
        run_id: String,
        conversation_id: String,
        cancel: CancellationToken,
    ) {
        // Acquire lane + global permit (cancellable while queued).
        let permit = match self.inner.queue.acquire(req.clone(), cancel.clone()).await {
            Ok(p) => p,
            Err(QueueError::Cancelled) => {
                self.finalize_terminal(
                    &run_id,
                    &conversation_id,
                    RunStatus::Cancelled,
                    None,
                    AgentEvent::RunCancelled {
                        run_id: run_id.clone(),
                        conversation_id: conversation_id.clone(),
                        seq: 0,
                        ts: 0,
                    },
                )
                .await;
                return;
            }
            Err(QueueError::Closed) => {
                self.finalize_terminal(
                    &run_id,
                    &conversation_id,
                    RunStatus::Failed,
                    Some("run queue closed"),
                    AgentEvent::RunFailed {
                        run_id: run_id.clone(),
                        conversation_id: conversation_id.clone(),
                        error: "run queue closed".into(),
                        seq: 0,
                        ts: 0,
                    },
                )
                .await;
                return;
            }
        };

        // Status -> running, emit RunStarted.
        if let Err(e) = self
            .inner
            .state
            .session_index
            .runs_set_status(&run_id, RunStatus::Running, None)
        {
            log::warn!("dispatch: set running status failed run_id={run_id}: {e}");
        }
        self.inner.events.emit(AgentEvent::RunStarted {
            run_id: run_id.clone(),
            conversation_id: conversation_id.clone(),
            seq: 0,
            ts: 0,
        });
        // Fire on_run_started observers (errors warn-logged, never abort).
        self.inner.hooks
            .run_on_run_started(&RunStartedContext {
                run_id: run_id.clone(),
                conversation_id: conversation_id.clone(),
                state: self.inner.state.clone(),
            })
            .await;

        // Build the per-run mpsc channel that run_chat pushes StreamEvents into.
        // `run_chat`'s `emit` calls `publish_stream`, which both broadcasts to
        // global UI subscribers AND sends to this tx. We drain rx in a forwarder
        // task that converts each StreamEvent to an AgentEvent tagged with our
        // run_id. The terminal StreamEvent::Done / Error are NOT converted here
        // (the dispatcher is the single source of truth for run status).
        let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
        let events = self.inner.events.clone();
        let run_id_fwd = run_id.clone();
        let fwd_handle = tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                if let Some(agent_ev) = stream_event_to_agent_event(&run_id_fwd, &ev) {
                    events.emit(agent_ev);
                }
            }
        });

        // Execute run_chat. Cancellation: the existing AppState.cancels map is
        // keyed by conversation_id and consulted by the agent loop's round
        // guards. We register our own CancellationToken there too so
        // `AppState::cancel(conversation_id)` (legacy path) cancels our run,
        // and `RunDispatcher::cancel(run_id)` (new path) does the same.
        self.inner
            .state
            .cancels
            .lock()
            .insert(conversation_id.clone(), cancel.clone());

        let result = run_chat(
            tx,
            self.inner.state.clone(),
            conversation_id.clone(),
            req.messages,
            req.enabled_skill_ids,
            req.agent_mode,
            req.lead_agent_id,
            req.tool_rounds_used_single_start,
            req.tool_rounds_used_supervisor_start,
            req.workspace_root,
            req.workspace_inherit_disabled,
        )
        .await;

        // Tear down: remove legacy cancel registration, drop permit, await
        // forwarder.
        self.inner.state.cancels.lock().remove(&conversation_id);
        drop(permit);
        let _ = fwd_handle.await;

        match result {
            Ok(()) => {
                self.finalize_terminal(
                    &run_id,
                    &conversation_id,
                    RunStatus::Finished,
                    None,
                    AgentEvent::RunFinished {
                        run_id: run_id.clone(),
                        conversation_id: conversation_id.clone(),
                        seq: 0,
                        ts: 0,
                    },
                )
                .await;
            }
            Err(e) => {
                let msg = format!("{e:#}");
                self.finalize_terminal(
                    &run_id,
                    &conversation_id,
                    RunStatus::Failed,
                    Some(msg.as_str()),
                    AgentEvent::RunFailed {
                        run_id: run_id.clone(),
                        conversation_id: conversation_id.clone(),
                        error: msg.clone(),
                        seq: 0,
                        ts: 0,
                    },
                )
                .await;
            }
        }
    }

    /// Persist terminal status, emit the terminal event, fire the matching
    /// observation hook, clean up the cancel registry, and finalize the
    /// per-run sequence counter.
    async fn finalize_terminal(
        &self,
        run_id: &str,
        conversation_id: &str,
        status: RunStatus,
        error: Option<&str>,
        terminal_event: AgentEvent,
    ) {
        if let Err(e) = self
            .inner
            .state
            .session_index
            .runs_set_status(run_id, status, error)
        {
            log::warn!("dispatch: set terminal status failed run_id={run_id}: {e}");
        }
        self.inner.events.emit(terminal_event);
        // Fire the matching observation hook. Errors are warn-logged inside the
        // registry runner; they never affect the run here.
        match status {
            RunStatus::Finished => {
                self.inner.hooks
                    .run_on_run_finished(&RunFinishedContext {
                        run_id: run_id.to_string(),
                        conversation_id: conversation_id.to_string(),
                        state: self.inner.state.clone(),
                    })
                    .await;
            }
            RunStatus::Failed => {
                self.inner.hooks
                    .run_on_run_failed(&RunFailedContext {
                        run_id: run_id.to_string(),
                        conversation_id: conversation_id.to_string(),
                        error: error.unwrap_or_default().to_string(),
                        state: self.inner.state.clone(),
                    })
                    .await;
            }
            RunStatus::Cancelled => {
                self.inner.hooks
                    .run_on_run_cancelled(&RunCancelledContext {
                        run_id: run_id.to_string(),
                        conversation_id: conversation_id.to_string(),
                        state: self.inner.state.clone(),
                    })
                    .await;
            }
            RunStatus::Queued | RunStatus::Running => {
                // Not a terminal state; no hook fired.
            }
        }
        self.inner.cancels.lock().remove(run_id);
        self.inner.events.finalize_run(run_id);
        log::info!(
            "dispatch: run_id={} conversation_id={} terminal status={}",
            run_id,
            conversation_id,
            status
        );
    }

    /// Cancel a run by id. If the run is queued, the queue acquire bails with
    /// `Cancelled`; if running, the cancellation token cancels the agent loop
    /// (same mechanism as `AppState::cancel`). No-op if the run is unknown /
    /// already terminal.
    pub fn cancel(&self, run_id: &str) {
        let Some(token) = self.inner.cancels.lock().remove(run_id) else {
            log::info!("dispatch: cancel no-op for unknown/terminal run_id={run_id}");
            return;
        };
        log::info!("dispatch: cancelling run_id={run_id}");
        token.cancel();
    }

    /// Block until the run reaches a terminal state, returning the outcome.
    /// If the run is unknown, returns an error. If already terminal, returns
    /// immediately (reads from the `runs` table).
    pub async fn wait(&self, run_id: &str) -> anyhow::Result<RunOutcome> {
        // Fast path: already terminal in the runs table.
        if let Ok(Some(rec)) = self.inner.state.session_index.runs_get(run_id) {
            if crate::conversation_store::runs::is_terminal_status_str(&rec.status) {
                return Ok(record_to_outcome(&rec));
            }
        }
        // Subscribe and wait for a terminal event.
        let mut rx = self.inner.events.subscribe();
        while let Ok(ev) = rx.recv().await {
            if ev.run_id() != run_id {
                continue;
            }
            match &*ev {
                AgentEvent::RunFinished { conversation_id, .. } => {
                    return Ok(RunOutcome::Finished {
                        run_id: run_id.to_string(),
                        conversation_id: conversation_id.clone(),
                    });
                }
                AgentEvent::RunFailed {
                    conversation_id,
                    error,
                    ..
                } => {
                    return Ok(RunOutcome::Failed {
                        run_id: run_id.to_string(),
                        conversation_id: conversation_id.clone(),
                        error: error.clone(),
                    });
                }
                AgentEvent::RunCancelled { conversation_id, .. } => {
                    return Ok(RunOutcome::Cancelled {
                        run_id: run_id.to_string(),
                        conversation_id: conversation_id.clone(),
                    });
                }
                _ => continue,
            }
        }
        // Bus closed without a terminal event — fall back to the runs table.
        if let Ok(Some(rec)) = self.inner.state.session_index.runs_get(run_id) {
            if crate::conversation_store::runs::is_terminal_status_str(&rec.status) {
                return Ok(record_to_outcome(&rec));
            }
        }
        anyhow::bail!("wait: run_id {} never reached a terminal state", run_id)
    }

    /// Subscribe to the live agent event bus. The receiver yields
    /// `Arc<AgentEvent>`s for ALL runs; callers filter by `run_id`. Late
    /// subscribers do not receive past events — combine with
    /// [`Self::run_status`] for a race-free terminal check.
    pub fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<
        std::sync::Arc<crate::agent_events::AgentEvent>,
    > {
        self.inner.events.subscribe()
    }

    /// Snapshot of a run's persisted state from the `runs` table, or `None`
    /// if the run id is unknown. Used by SSE / polling clients to recover
    /// the terminal state of a run that already finished before they
    /// subscribed to the bus.
    pub fn run_status(
        &self,
        run_id: &str,
    ) -> Option<crate::conversation_store::runs::RunRecord> {
        self.inner
            .state
            .session_index
            .runs_get(run_id)
            .ok()
            .flatten()
    }

    /// Convenience for in-process internal triggers (background tasks that
    /// drive a conversation turn). Builds a [`TriggerRequest`] with
    /// `trigger_source = Internal` and the given `internal_label`
    /// (recorded in `trigger_meta`), then dispatches it.
    ///
    /// Note: existing internal background work (curator LLM pass, memory
    /// review) is NOT conversation-turn shaped — they run bespoke LLM passes
    /// outside `run_chat` and are intentionally NOT routed here. This helper
    /// is for future internal triggers that produce a normal conversation
    /// turn (e.g. a boot checklist that posts a user message and runs the
    /// agent loop).
    pub async fn dispatch_internal(
        &self,
        conversation_id: String,
        messages: Vec<ChatMessage>,
        internal_label: impl Into<String>,
    ) -> anyhow::Result<RunHandle> {
        let req = TriggerRequest {
            run_id: None,
            idempotency_key: None,
            conversation_id: Some(conversation_id),
            trigger_source: TriggerSource::Internal,
            trigger_meta: TriggerMeta {
                internal_label: Some(internal_label.into()),
                ..TriggerMeta::empty()
            },
            lane: None,
            messages,
            enabled_skill_ids: Vec::new(),
            agent_mode: None,
            lead_agent_id: None,
            tool_rounds_used_single_start: 0,
            tool_rounds_used_supervisor_start: 0,
            workspace_root: String::new(),
            workspace_inherit_disabled: None,
            deliver: DeliverTarget::None,
            web_session_auth: None,
        };
        self.dispatch(req).await
    }
}

fn record_to_outcome(rec: &crate::conversation_store::runs::RunRecord) -> RunOutcome {
    match rec.status.as_str() {
        "finished" => RunOutcome::Finished {
            run_id: rec.run_id.clone(),
            conversation_id: rec.conversation_id.clone(),
        },
        "failed" => RunOutcome::Failed {
            run_id: rec.run_id.clone(),
            conversation_id: rec.conversation_id.clone(),
            error: rec.error.clone().unwrap_or_default(),
        },
        "cancelled" => RunOutcome::Cancelled {
            run_id: rec.run_id.clone(),
            conversation_id: rec.conversation_id.clone(),
        },
        other => RunOutcome::Failed {
            run_id: rec.run_id.clone(),
            conversation_id: rec.conversation_id.clone(),
            error: format!("unexpected runs table status: {other}"),
        },
    }
}
