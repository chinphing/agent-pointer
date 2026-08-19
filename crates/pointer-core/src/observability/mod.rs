//! Run observability: trace semantics + async non-blocking pipeline +
//! pluggable exporters (plan §7).
//!
//! - [`trace`]: TraceEvent / SpanKind / SpanStatus (OTel-aligned semantics)
//! - [`redact`]: payload redaction (runs in the background task)
//! - [`exporters`]: TraceExporter trait + ExporterRegistry + LogExporter
//! - [`otlp`][]: OtlpExporter (OTLP/HTTP + JSON; env-var activated, P4 ③)
//! - [`pipeline`]: bounded-channel pipeline with drop counting

pub mod exporters;
pub mod otlp;
pub mod pipeline;
pub mod redact;
pub mod trace;

pub use exporters::{ExporterRegistry, LogExporter, TraceExporter};
pub use otlp::OtlpExporter;
pub use pipeline::{start, TraceBus, CAPTURE_MAX_BYTES, TRACE_CHANNEL_CAPACITY};
pub use redact::redact_value;
pub use trace::{capture_truncate, now_ms, SpanKind, SpanStatus, TraceError, TraceEvent};

/// Convenience: build a default pipeline and spawn its background task. Used
/// by hosts that want observability with zero config.
///
/// Registers the built-in `LogExporter`; when the standard OTel env vars are
/// set (`OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` / `OTEL_EXPORTER_OTLP_ENDPOINT`,
/// protocol `http/json`) an [`OtlpExporter`] is appended.
pub fn start_default() -> TraceBus {
    let mut reg = ExporterRegistry::new();
    reg.register(std::sync::Arc::new(LogExporter));
    if let Some(otlp) = OtlpExporter::from_env() {
        reg.register(std::sync::Arc::new(otlp));
    }
    pipeline::start(std::sync::Arc::new(reg))
}
