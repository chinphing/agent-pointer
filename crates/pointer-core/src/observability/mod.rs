//! Run observability: trace semantics + async non-blocking pipeline +
//! pluggable exporters (plan §7).
//!
//! - [`trace`]: TraceEvent / SpanKind / SpanStatus (OTel-aligned semantics)
//! - [`redact`]: payload redaction (runs in the background task)
//! - [`exporters`]: TraceExporter trait + ExporterRegistry + LogExporter
//! - [`pipeline`]: bounded-channel pipeline with drop counting

pub mod exporters;
pub mod pipeline;
pub mod redact;
pub mod trace;

pub use exporters::{ExporterRegistry, LogExporter, TraceExporter};
pub use pipeline::{start, TraceBus, CAPTURE_MAX_BYTES, TRACE_CHANNEL_CAPACITY};
pub use redact::redact_value;
pub use trace::{capture_truncate, now_ms, SpanKind, SpanStatus, TraceError, TraceEvent};

/// Convenience: build a default pipeline (LogExporter only) and spawn its
/// background task. Used by hosts that want observability with zero config.
pub fn start_default() -> TraceBus {
    let mut reg = ExporterRegistry::new();
    reg.register(std::sync::Arc::new(LogExporter));
    pipeline::start(std::sync::Arc::new(reg))
}
