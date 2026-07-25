//! Typed agent run event bus (per-run sequenced broadcast).
//!
//! [`AgentEvent`] is the canonical event payload for a run lifecycle: queueing,
//! start, assistant deltas, tool calls, finish/fail/cancel. Each event carries a
//! `run_id` and a per-run `seq` (monotonic, starting at 1) so subscribers can
//! order and deduplicate across transports (Tauri IPC, web SSE, future SDK).
//!
//! Design borrows from openclaw `emitAgentEvent` (in-process pub/sub with seq)
//! and hermes `stream_events` (typed structured chunks). The bus is a
//! `tokio::sync::broadcast` channel; late subscribers only receive events
//! emitted after they subscribe. Per-run history is persisted in the `runs`
//! table via [`RunDispatcher`](crate::dispatcher::RunDispatcher), not here.
//!
//! Phase 1: the bus coexists with [`crate::stream_broadcast`] (legacy UI
//! channel). [`bridge::stream_event_to_agent_event`] converts the existing
//! [`crate::models::StreamEvent`] stream into [`AgentEvent`]s so `run_chat`
//! does not need to be modified to emit AgentEvents directly.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::models::StreamEvent;

/// Default channel capacity for the broadcast bus. Events past this capacity
/// are dropped for slow subscribers; per-run persistence (runs table) is the
/// source of truth, not the bus.
const BUS_CAPACITY: usize = 1024;

/// Canonical run lifecycle event. Variants mirror the openclaw `agent` event
/// stream shape (lifecycle / assistant / tool / error) plus queue-specific
/// events (`RunQueued`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentEvent {
    /// Run accepted by dispatcher, waiting for a lane slot.
    RunQueued { run_id: String, seq: u64, ts: u64 },
    /// Run left the queue and started executing (`run_chat` invoked).
    RunStarted {
        run_id: String,
        conversation_id: String,
        seq: u64,
        ts: u64,
    },
    /// Assistant text delta (mirrors `StreamEvent::Delta`).
    AssistantDelta {
        run_id: String,
        message_id: String,
        text: String,
        seq: u64,
    },
    /// A tool call started (mirrors `StreamEvent::ToolCallStart`).
    ToolCallStart {
        run_id: String,
        message_id: String,
        tool_call_id: String,
        tool_name: String,
        seq: u64,
    },
    /// A tool call finished with a status (`ok` / `error` / `cancelled`).
    ToolCallResult {
        run_id: String,
        message_id: String,
        tool_call_id: String,
        status: String,
        seq: u64,
    },
    /// Run completed successfully.
    RunFinished {
        run_id: String,
        conversation_id: String,
        seq: u64,
        ts: u64,
    },
    /// Run failed with an error message.
    RunFailed {
        run_id: String,
        conversation_id: String,
        error: String,
        seq: u64,
        ts: u64,
    },
    /// Run cancelled by the user or host.
    RunCancelled {
        run_id: String,
        conversation_id: String,
        seq: u64,
        ts: u64,
    },
}

impl AgentEvent {
    pub fn run_id(&self) -> &str {
        match self {
            AgentEvent::RunQueued { run_id, .. }
            | AgentEvent::RunStarted { run_id, .. }
            | AgentEvent::AssistantDelta { run_id, .. }
            | AgentEvent::ToolCallStart { run_id, .. }
            | AgentEvent::ToolCallResult { run_id, .. }
            | AgentEvent::RunFinished { run_id, .. }
            | AgentEvent::RunFailed { run_id, .. }
            | AgentEvent::RunCancelled { run_id, .. } => run_id,
        }
    }

    pub fn seq(&self) -> u64 {
        match self {
            AgentEvent::RunQueued { seq, .. }
            | AgentEvent::RunStarted { seq, .. }
            | AgentEvent::AssistantDelta { seq, .. }
            | AgentEvent::ToolCallStart { seq, .. }
            | AgentEvent::ToolCallResult { seq, .. }
            | AgentEvent::RunFinished { seq, .. }
            | AgentEvent::RunFailed { seq, .. }
            | AgentEvent::RunCancelled { seq, .. } => *seq,
        }
    }
}

/// In-process agent event bus. One bus per host process (Tauri / server),
/// owned by [`crate::dispatcher::RunDispatcher`].
#[derive(Clone)]
pub struct AgentEventBus {
    tx: broadcast::Sender<Arc<AgentEvent>>,
    seqs: Arc<Mutex<HashMap<String, u64>>>,
}

impl AgentEventBus {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(BUS_CAPACITY);
        Self {
            tx,
            seqs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Subscribe to all future events. Late subscribers do not receive past
    /// events; query the `runs` table for history.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<AgentEvent>> {
        self.tx.subscribe()
    }

    /// Emit an event, assigning the next per-run `seq`. Returns the assigned
    /// sequence number. Sequence starts at 1.
    pub fn emit(&self, mut ev: AgentEvent) -> u64 {
        let run_id = ev.run_id().to_string();
        let next_seq = {
            let mut seqs = self.seqs.lock();
            let cur = seqs.get(&run_id).copied().unwrap_or(0);
            let next = cur.saturating_add(1);
            seqs.insert(run_id.clone(), next);
            next
        };
        // Patch seq + ts on every variant (ts is set here, not by callers).
        patch_seq(&mut ev, next_seq);
        patch_ts(&mut ev, now_ms());
        let _ = self.tx.send(Arc::new(ev));
        next_seq
    }

    /// Drop the per-run sequence counter (called when a run reaches a terminal
    /// state to keep the map bounded).
    pub fn finalize_run(&self, run_id: &str) {
        self.seqs.lock().remove(run_id);
    }
}

impl Default for AgentEventBus {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn patch_seq(ev: &mut AgentEvent, seq: u64) {
    match ev {
        AgentEvent::RunQueued { seq: s, .. }
        | AgentEvent::RunStarted { seq: s, .. }
        | AgentEvent::AssistantDelta { seq: s, .. }
        | AgentEvent::ToolCallStart { seq: s, .. }
        | AgentEvent::ToolCallResult { seq: s, .. }
        | AgentEvent::RunFinished { seq: s, .. }
        | AgentEvent::RunFailed { seq: s, .. }
        | AgentEvent::RunCancelled { seq: s, .. } => *s = seq,
    }
}

fn patch_ts(ev: &mut AgentEvent, ts: u64) {
    match ev {
        AgentEvent::RunQueued { ts: t, .. }
        | AgentEvent::RunStarted { ts: t, .. }
        | AgentEvent::RunFinished { ts: t, .. }
        | AgentEvent::RunFailed { ts: t, .. }
        | AgentEvent::RunCancelled { ts: t, .. } => *t = ts,
        // Deltas / tool events are high-frequency; ts is derivable from arrival time.
        AgentEvent::AssistantDelta { .. }
        | AgentEvent::ToolCallStart { .. }
        | AgentEvent::ToolCallResult { .. } => {}
    }
}

/// Bridge from legacy [`StreamEvent`] (per-run mpsc) to typed [`AgentEvent`].
///
/// Returns `None` for stream events that have no agent-event analogue (e.g.
/// UI-only toasts, task-board updates, IM session forks). `run_id` is supplied
/// by the dispatcher since `StreamEvent` does not carry it.
pub fn stream_event_to_agent_event(run_id: &str, ev: &StreamEvent) -> Option<AgentEvent> {
    match ev {
        StreamEvent::Delta { message_id, text } => Some(AgentEvent::AssistantDelta {
            run_id: run_id.to_string(),
            message_id: message_id.clone(),
            text: text.clone(),
            seq: 0,
        }),
        StreamEvent::ToolCallStart {
            message_id,
            tool_call,
            ..
        } => Some(AgentEvent::ToolCallStart {
            run_id: run_id.to_string(),
            message_id: message_id.clone(),
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            seq: 0,
        }),
        StreamEvent::ToolCallStatus {
            message_id,
            tool_call_id,
            status,
            ..
        } => Some(AgentEvent::ToolCallResult {
            run_id: run_id.to_string(),
            message_id: message_id.clone(),
            tool_call_id: tool_call_id.clone(),
            status: status.clone(),
            seq: 0,
        }),
        // Run-terminal events are emitted explicitly by the dispatcher, not
        // derived from `Done`/`Error` here, so we keep a single source of truth
        // for run status transitions.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn emit_assigns_monotonic_seq_per_run() {
        let bus = AgentEventBus::new();
        let mut rx = bus.subscribe();
        let run_id = "r1";

        let s1 = bus.emit(AgentEvent::RunStarted {
            run_id: run_id.to_string(),
            conversation_id: "c1".into(),
            seq: 0,
            ts: 0,
        });
        let s2 = bus.emit(AgentEvent::AssistantDelta {
            run_id: run_id.to_string(),
            message_id: "m1".into(),
            text: "hi".into(),
            seq: 0,
        });
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);

        let ev1 = rx.recv().await.unwrap();
        let ev2 = rx.recv().await.unwrap();
        assert_eq!(ev1.seq(), 1);
        assert_eq!(ev2.seq(), 2);
        bus.finalize_run(run_id);
    }

    #[tokio::test]
    async fn seqs_are_independent_per_run() {
        let bus = AgentEventBus::new();
        let s_a = bus.emit(AgentEvent::RunStarted {
            run_id: "a".into(),
            conversation_id: "c".into(),
            seq: 0,
            ts: 0,
        });
        let s_b = bus.emit(AgentEvent::RunStarted {
            run_id: "b".into(),
            conversation_id: "c".into(),
            seq: 0,
            ts: 0,
        });
        assert_eq!(s_a, 1);
        assert_eq!(s_b, 1);
        bus.finalize_run("a");
        bus.finalize_run("b");
    }
}
