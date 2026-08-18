//! Exporter plugin interface (plan §7.3). Exporters are pluggable; the
//! built-in set is LogExporter (default) and RealtimeExporter (default).
//! A single exporter failure must not affect others (fault isolation).

use std::sync::Arc;

use async_trait::async_trait;

use super::trace::TraceEvent;

#[async_trait]
pub trait TraceExporter: Send + Sync {
    fn name(&self) -> &str;
    /// Export a batch. Errors are isolated per exporter (logged, not propagated).
    async fn export(&self, batch: Vec<TraceEvent>) -> Result<(), String>;
    async fn shutdown(&self) {}
}

/// Registry of exporters. `export_batch` fans out to each exporter with
/// per-exporter error isolation: a failing exporter is logged and skipped.
#[derive(Default)]
pub struct ExporterRegistry {
    exporters: Vec<Arc<dyn TraceExporter>>,
}

impl ExporterRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, exporter: Arc<dyn TraceExporter>) {
        self.exporters.push(exporter);
    }

    pub fn is_empty(&self) -> bool {
        self.exporters.is_empty()
    }

    /// Fan out a batch to all exporters. Returns the number of exporters
    /// that failed (each failure is logged, never propagated).
    pub async fn export_batch(&self, batch: &[TraceEvent]) -> usize {
        let mut failed = 0;
        for exporter in &self.exporters {
            if let Err(e) = exporter.export(batch.to_vec()).await {
                failed += 1;
                log::warn!("trace exporter '{}' failed: {e}", exporter.name());
            }
        }
        failed
    }

    pub async fn shutdown_all(&self) {
        for exporter in &self.exporters {
            exporter.shutdown().await;
        }
    }
}

/// Built-in default exporter: one structured log line per span.
pub struct LogExporter;

#[async_trait]
impl TraceExporter for LogExporter {
    fn name(&self) -> &str {
        "log"
    }

    async fn export(&self, batch: Vec<TraceEvent>) -> Result<(), String> {
        for ev in &batch {
            log::info!(
                "trace span_end trace_id={} span_id={} parent={} kind={} name={} status={} duration_ms={} attrs={}",
                ev.trace_id,
                ev.span_id,
                ev.parent_span_id.as_deref().unwrap_or("-"),
                ev.kind.as_str(),
                ev.name,
                ev.status.as_str(),
                ev.duration_ms.map(|d| d.to_string()).unwrap_or_else(|| "-".into()),
                ev.attributes
            );
            if let Some(err) = &ev.error {
                log::warn!(
                    "trace span_error trace_id={} span_id={} code={} message={}",
                    ev.trace_id,
                    ev.span_id,
                    err.code,
                    err.message
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::trace::{SpanKind, TraceEvent};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingExporter {
        name: &'static str,
        calls: AtomicUsize,
        fail: bool,
    }

    #[async_trait]
    impl TraceExporter for CountingExporter {
        fn name(&self) -> &str {
            self.name
        }
        async fn export(&self, batch: Vec<TraceEvent>) -> Result<(), String> {
            self.calls.fetch_add(batch.len(), Ordering::SeqCst);
            if self.fail {
                Err("boom".into())
            } else {
                Ok(())
            }
        }
    }

    fn sample() -> TraceEvent {
        let mut ev = TraceEvent::new("r1", "s1", SpanKind::ToolCall, "t");
        ev.end();
        ev
    }

    #[tokio::test]
    async fn fault_isolation_skips_failing_exporter() {
        let mut reg = ExporterRegistry::new();
        let bad = Arc::new(CountingExporter {
            name: "bad",
            calls: AtomicUsize::new(0),
            fail: true,
        });
        let good = Arc::new(CountingExporter {
            name: "good",
            calls: AtomicUsize::new(0),
            fail: false,
        });
        reg.register(bad.clone());
        reg.register(good.clone());

        let failed = reg.export_batch(&[sample()]).await;
        assert_eq!(failed, 1);
        assert_eq!(good.calls.load(Ordering::SeqCst), 1);
        assert_eq!(bad.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn empty_registry_is_noop() {
        let reg = ExporterRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.export_batch(&[sample()]).await, 0);
    }
}
