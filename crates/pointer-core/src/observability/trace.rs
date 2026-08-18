//! Trace semantics for run observability (plan §7.1).
//!
//! `trace_id` = `run_id` (one trace per run). Spans form a tree via
//! `span_id` / `parent_span_id`. The model maps 1:1 to OpenTelemetry
//! (trace_id / span_id / parent_span_id / attributes / status) so a future
//! `OtlpExporter` is a thin adapter.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    Run,
    AgentLoop,
    LlmCall,
    ToolCall,
    Approval,
    Retry,
    Hook,
    McpRequest,
    PluginLoad,
    PluginHealthCheck,
}

impl SpanKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SpanKind::Run => "run",
            SpanKind::AgentLoop => "agent_loop",
            SpanKind::LlmCall => "llm_call",
            SpanKind::ToolCall => "tool_call",
            SpanKind::Approval => "approval",
            SpanKind::Retry => "retry",
            SpanKind::Hook => "hook",
            SpanKind::McpRequest => "mcp_request",
            SpanKind::PluginLoad => "plugin_load",
            SpanKind::PluginHealthCheck => "plugin_health_check",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanStatus {
    Ok,
    Error,
    Cancelled,
}

impl SpanStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SpanStatus::Ok => "ok",
            SpanStatus::Error => "error",
            SpanStatus::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceError {
    pub code: String,
    pub message: String,
}

/// One span record. `input` / `output` are capture-time truncated payloads;
/// full redaction happens in the background pipeline task (plan §7.2).
#[derive(Debug, Clone)]
pub struct TraceEvent {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub run_id: String,
    pub conversation_id: String,
    pub kind: SpanKind,
    pub name: String,
    pub status: SpanStatus,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub duration_ms: Option<u64>,
    /// Free-form attributes (plugin_id, tool_name, tokens_in/out, ...).
    pub attributes: Value,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub error: Option<TraceError>,
}

impl TraceEvent {
    pub fn new(
        trace_id: impl Into<String>,
        span_id: impl Into<String>,
        kind: SpanKind,
        name: impl Into<String>,
    ) -> Self {
        let trace_id = trace_id.into();
        Self {
            trace_id: trace_id.clone(),
            span_id: span_id.into(),
            parent_span_id: None,
            run_id: trace_id,
            conversation_id: String::new(),
            kind,
            name: name.into(),
            status: SpanStatus::Ok,
            started_at_ms: now_ms(),
            ended_at_ms: None,
            duration_ms: None,
            attributes: Value::Object(Default::default()),
            input: None,
            output: None,
            error: None,
        }
    }

    pub fn end(&mut self) {
        let now = now_ms();
        self.ended_at_ms = Some(now);
        self.duration_ms = Some((now - self.started_at_ms).max(0) as u64);
    }

    pub fn set_error(&mut self, code: impl Into<String>, message: impl Into<String>) {
        self.status = SpanStatus::Error;
        self.error = Some(TraceError {
            code: code.into(),
            message: message.into(),
        });
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Capture-time truncation: keep span-end payload capture cheap (plan §7.2).
/// Values larger than `max_bytes` when serialized are replaced by a marker.
pub fn capture_truncate(value: Value, max_bytes: usize) -> Value {
    if value.to_string().len() > max_bytes {
        Value::String(format!(
            "[payload {} bytes truncated]",
            value.to_string().len()
        ))
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_computes_duration() {
        let mut ev = TraceEvent::new("run-1", "s1", SpanKind::ToolCall, "file_read");
        ev.started_at_ms = 1000;
        ev.end();
        assert!(ev.duration_ms.is_some());
        assert_eq!(
            ev.ended_at_ms.unwrap(),
            ev.started_at_ms + ev.duration_ms.unwrap() as i64
        );
    }

    #[test]
    fn capture_truncate_caps_large_payloads() {
        let big = Value::String("x".repeat(10_000));
        let out = capture_truncate(big, 1024);
        assert!(out.as_str().unwrap().contains("truncated"));
        let small = Value::String("ok".into());
        assert_eq!(capture_truncate(small.clone(), 1024), small);
    }
}
