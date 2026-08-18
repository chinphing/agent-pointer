//! Async non-blocking trace pipeline (plan §7.2).
//!
//! Instrumentation points call [`TraceBus::emit`], which is a `try_send` on a
//! bounded channel: nanosecond-scale, never blocks the agent loop. When the
//! channel is full the event is dropped and counted (the drop counter is
//! itself exposed as a metric). A single background task batches events,
//! redacts them, and fans out to the [`ExporterRegistry`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc;

use super::exporters::ExporterRegistry;
use super::redact::redact_event_in_place;
use super::trace::TraceEvent;

/// Channel capacity. Sizing: a busy run emits a few dozen spans; 4096 is
/// orders of magnitude above any realistic burst.
pub const TRACE_CHANNEL_CAPACITY: usize = 4096;

/// Batch size for the background consumer.
const BATCH_SIZE: usize = 64;

/// Max wait for a full batch before flushing (keeps latency low).
const BATCH_TIMEOUT_MS: u64 = 250;

/// Max bytes for span-end payload capture (cheap truncation at capture time).
pub const CAPTURE_MAX_BYTES: usize = 8 * 1024;

pub struct TracePipeline {
    tx: mpsc::Sender<TraceEvent>,
    dropped: Arc<AtomicU64>,
}

impl TracePipeline {
    /// Non-blocking emit. Returns false when the channel is full (dropped).
    pub fn emit(&self, event: TraceEvent) -> bool {
        match self.tx.try_send(event) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// Shared handle stored on AppState; cheap to clone (Arc inside).
#[derive(Clone)]
pub struct TraceBus {
    inner: Arc<TracePipeline>,
}

impl TraceBus {
    pub fn emit(&self, event: TraceEvent) -> bool {
        self.inner.emit(event)
    }

    pub fn dropped_count(&self) -> u64 {
        self.inner.dropped_count()
    }

    /// A bus whose `emit` always returns false (dropped). Used when no Tokio
    /// runtime is available at construction time (e.g. non-async contexts):
    /// the receiver is dropped immediately, so every `try_send` returns Closed.
    pub fn noop() -> Self {
        let (tx, _rx) = mpsc::channel::<TraceEvent>(1);
        let dropped = Arc::new(AtomicU64::new(0));
        Self {
            inner: Arc::new(TracePipeline { tx, dropped }),
        }
    }
}

pub struct TracePipelineHandle {
    rx: mpsc::Receiver<TraceEvent>,
    exporters: Arc<ExporterRegistry>,
    dropped: Arc<AtomicU64>,
}

impl TracePipelineHandle {
    /// Consume until the channel closes, flushing in batches.
    pub async fn run(self) {
        let Self {
            mut rx,
            exporters,
            dropped,
        } = self;
        let mut batch: Vec<TraceEvent> = Vec::with_capacity(BATCH_SIZE);
        loop {
            // First event: wait with timeout.
            let first = tokio::select! {
                ev = rx.recv() => ev,
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(BATCH_TIMEOUT_MS)) => {
                    if !batch.is_empty() {
                        flush(&batch, &exporters).await;
                        batch.clear();
                    }
                    continue;
                }
            };
            match first {
                Some(ev) => batch.push(ev),
                None => {
                    if !batch.is_empty() {
                        flush(&batch, &exporters).await;
                    }
                    break;
                }
            }
            // Fill the rest of the batch without blocking.
            while batch.len() < BATCH_SIZE {
                match rx.try_recv() {
                    Ok(ev) => batch.push(ev),
                    Err(_) => break,
                }
            }
            flush(&batch, &exporters).await;
            batch.clear();
        }
        let _ = dropped; // drop counter lives on the pipeline side
    }
}

async fn flush(batch: &[TraceEvent], exporters: &ExporterRegistry) {
    // Redaction happens here, off the hot path.
    let mut owned: Vec<TraceEvent> = batch.to_vec();
    for ev in owned.iter_mut() {
        redact_event_in_place(ev);
    }
    exporters.export_batch(&owned).await;
}

/// Build the pipeline and spawn the background consumer task.
/// Returns the shared [`TraceBus`] handle for instrumentation points.
pub fn start(exporters: Arc<ExporterRegistry>) -> TraceBus {
    let (tx, rx) = mpsc::channel(TRACE_CHANNEL_CAPACITY);
    let dropped = Arc::new(AtomicU64::new(0));
    let bus = TraceBus {
        inner: Arc::new(TracePipeline {
            tx,
            dropped: dropped.clone(),
        }),
    };
    let handle = TracePipelineHandle {
        rx,
        exporters,
        dropped,
    };
    tokio::spawn(handle.run());
    bus
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::exporters::{ExporterRegistry, TraceExporter};
    use crate::observability::trace::{SpanKind, TraceEvent};
    use std::sync::atomic::AtomicUsize;

    struct CollectingExporter {
        received: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl TraceExporter for CollectingExporter {
        fn name(&self) -> &str {
            "collecting"
        }
        async fn export(&self, batch: Vec<TraceEvent>) -> Result<(), String> {
            self.received.fetch_add(batch.len(), Ordering::SeqCst);
            Ok(())
        }
    }

    fn sample(i: u64) -> TraceEvent {
        let mut ev = TraceEvent::new("r1", format!("s{i}"), SpanKind::ToolCall, "t");
        ev.end();
        ev
    }

    #[tokio::test]
    async fn emit_delivers_events_to_exporter() {
        let collecting = Arc::new(CollectingExporter {
            received: AtomicUsize::new(0),
        });
        let mut reg = ExporterRegistry::new();
        reg.register(collecting.clone());
        let bus = start(Arc::new(reg));

        for i in 0..10 {
            assert!(bus.emit(sample(i)));
        }
        // Wait for the background task to flush (batch timeout 250ms).
        for _ in 0..40 {
            if collecting.received.load(Ordering::SeqCst) >= 10 {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        assert_eq!(collecting.received.load(Ordering::SeqCst), 10);
    }

    #[tokio::test]
    async fn channel_full_drops_and_counts() {
        let reg = ExporterRegistry::new();
        let bus = start(Arc::new(reg));
        // Fill the channel without a consumer draining fast enough:
        // the background task exists but we outpace it by emitting a lot.
        let mut sent = 0usize;
        for i in 0..(TRACE_CHANNEL_CAPACITY * 4) {
            if bus.emit(sample(i as u64)) {
                sent += 1;
            }
        }
        assert!(sent < TRACE_CHANNEL_CAPACITY * 4);
        assert!(bus.dropped_count() > 0);
    }

    #[tokio::test]
    async fn redaction_applied_before_export() {
        struct InspectingExporter {
            seen: std::sync::Mutex<Option<serde_json::Value>>,
        }
        #[async_trait::async_trait]
        impl TraceExporter for InspectingExporter {
            fn name(&self) -> &str {
                "inspecting"
            }
            async fn export(&self, batch: Vec<TraceEvent>) -> Result<(), String> {
                if let Some(ev) = batch.first() {
                    *self.seen.lock().unwrap() = ev.input.clone();
                }
                Ok(())
            }
        }
        let inspecting = Arc::new(InspectingExporter {
            seen: std::sync::Mutex::new(None),
        });
        let mut reg = ExporterRegistry::new();
        reg.register(inspecting.clone());
        let bus = start(Arc::new(reg));

        let mut ev = sample(1);
        ev.input = Some(serde_json::json!({ "api_token": "secret-value", "ok": true }));
        assert!(bus.emit(ev));

        for _ in 0..40 {
            if inspecting.seen.lock().unwrap().is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        let seen = inspecting.seen.lock().unwrap().take().unwrap();
        assert_eq!(seen["api_token"], serde_json::json!("[REDACTED]"));
        assert_eq!(seen["ok"], serde_json::json!(true));
    }
}
