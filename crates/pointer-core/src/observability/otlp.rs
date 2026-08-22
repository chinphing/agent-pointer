//! OTLP exporter (plan §7.3, P4 ③).
//!
//! Exports [`TraceEvent`] batches to an OpenTelemetry Collector via the
//! **OTLP/HTTP + protobuf** protocol (`POST {endpoint}/v1/traces`,
//! `Content-Type: application/x-protobuf`). Phoenix's OTLP receiver only
//! accepts protobuf (it answers 415 to JSON), so the payload is encoded with
//! the prost-generated `opentelemetry-proto` message types (trimmed features:
//! no tonic transport).
//!
//! Activation follows the standard OpenTelemetry environment variables:
//! - `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` — full `.../v1/traces` endpoint
//! - `OTEL_EXPORTER_OTLP_ENDPOINT` — base endpoint (signal path appended)
//! - `OTEL_EXPORTER_OTLP_PROTOCOL` — `http/protobuf` (default); `http/json`
//!   and `grpc` are logged and skipped
//! - `OTEL_SERVICE_NAME` — default `pointer-app`
//!
//! OpenInference semantics: every span carries `openinference.span.kind`
//! (CHAIN / LLM / TOOL) plus `gen_ai.*` semconv attributes (model, token
//! usage, conversation id, tool name) so Phoenix renders LLM/tool details.
//!
//! Export failures are fail-open: the pipeline logs them and continues
//! (fault isolation is handled by [`ExporterRegistry::export_batch`]).
//!
//! Trace/span ids in Pointer are strings (`run_id` for trace, uuid for span).
//! OTLP requires fixed-width byte ids: hex-decodable strings (uuid form) are
//! decoded, anything else falls back to a stable sha256 derivation.

use std::env;

use async_trait::async_trait;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{
    any_value::Value, AnyValue, ArrayValue, InstrumentationScope, KeyValue, KeyValueList,
};
use opentelemetry_proto::tonic::resource::v1::Resource;
use opentelemetry_proto::tonic::trace::v1::{
    span::SpanKind as OtlpSpanKind, ResourceSpans, ScopeSpans, Span, Status,
};
use prost::Message;
use serde_json::Value as JsonValue;

use sha2::{Digest, Sha256};

use super::exporters::TraceExporter;
use super::trace::{SpanKind, SpanStatus, TraceEvent};

/// Default service name when `OTEL_SERVICE_NAME` is not set.
const DEFAULT_SERVICE_NAME: &str = "pointer-app";

/// HTTP client request timeout for a single export call.
const EXPORT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// OTLP/HTTP JSON exporter.
pub struct OtlpExporter {
    endpoint: String,
    service_name: String,
    client: reqwest::Client,
}

impl OtlpExporter {
    /// Create an exporter for `endpoint` (must be the full `/v1/traces` URL).
    pub fn new(endpoint: impl Into<String>, service_name: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            service_name: service_name.into(),
            client: reqwest::Client::builder()
                .timeout(EXPORT_TIMEOUT)
                .build()
                .unwrap_or_default(),
        }
    }

    /// Build from standard OTel env vars. `None` when not configured (or the
    /// configured protocol is not `http/json`).
    pub fn from_env() -> Option<Self> {
        Self::from_env_impl(|key| env::var(key).ok().filter(|v| !v.trim().is_empty()))
    }

    /// Env resolution as a pure function for testability.
    fn from_env_impl(get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        if let Some(proto) = get("OTEL_EXPORTER_OTLP_PROTOCOL") {
            let proto = proto.trim().to_ascii_lowercase();
            if !proto.is_empty() && proto != "http/protobuf" {
                log::warn!(
                    "OtlpExporter: unsupported OTEL_EXPORTER_OTLP_PROTOCOL={proto} \
                     (only http/protobuf); skipping OTLP export"
                );
                return None;
            }
        }
        let endpoint = get("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT").or_else(|| {
            get("OTEL_EXPORTER_OTLP_ENDPOINT")
                .map(|base| format!("{}/v1/traces", base.trim_end_matches('/')))
        })?;
        let service_name =
            get("OTEL_SERVICE_NAME").unwrap_or_else(|| DEFAULT_SERVICE_NAME.to_string());
        Some(Self::new(endpoint, service_name))
    }
}

#[async_trait]
impl TraceExporter for OtlpExporter {
    fn name(&self) -> &str {
        "otlp"
    }

    async fn export(&self, batch: Vec<TraceEvent>) -> Result<(), String> {
        if batch.is_empty() {
            return Ok(());
        }
        let payload = build_export_request(&batch, &self.service_name);
        let resp = self
            .client
            .post(&self.endpoint)
            .header(reqwest::header::CONTENT_TYPE, "application/x-protobuf")
            .body(payload)
            .send()
            .await
            .map_err(|e| format!("otlp http request failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("otlp http status {}", resp.status()));
        }
        Ok(())
    }
}

/// Serialize a batch into an `ExportTraceServiceRequest` protobuf body
/// (OTLP/HTTP + protobuf encoding).
pub fn build_export_request(batch: &[TraceEvent], service_name: &str) -> Vec<u8> {
    let spans: Vec<Span> = batch.iter().map(span_to_proto).collect();
    let req = ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            resource: Some(Resource {
                attributes: vec![KeyValue {
                    key: "service.name".into(),
                    value: Some(AnyValue {
                        value: Some(Value::StringValue(service_name.to_string())),
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            scope_spans: vec![ScopeSpans {
                scope: Some(InstrumentationScope {
                    name: "pointer-core".into(),
                    version: env!("CARGO_PKG_VERSION").into(),
                    ..Default::default()
                }),
                spans,
                ..Default::default()
            }],
            ..Default::default()
        }],
    };
    req.encode_to_vec()
}

fn span_to_proto(ev: &TraceEvent) -> Span {
    let start_ns = (ev.started_at_ms.max(0) as u64).saturating_mul(1_000_000);
    let end_ms = ev.ended_at_ms.unwrap_or(ev.started_at_ms);
    let end_ns = (end_ms.max(0) as u64).saturating_mul(1_000_000);

    let mut attrs: Vec<KeyValue> = Vec::new();
    // OpenInference semantics: Phoenix reads the span kind from this attribute.
    push_attr(
        &mut attrs,
        "openinference.span.kind",
        openinference_kind(ev.kind),
    );
    push_attr(&mut attrs, "span.kind", ev.kind.as_str());
    push_attr(&mut attrs, "run_id", &ev.run_id);
    if !ev.conversation_id.is_empty() {
        push_attr(&mut attrs, "conversation_id", &ev.conversation_id);
        // OTel GenAI semconv: Phoenix synthesizes session_id from this.
        push_attr(&mut attrs, "gen_ai.conversation.id", &ev.conversation_id);
    }
    if let Some(d) = ev.duration_ms {
        push_attr(&mut attrs, "duration_ms", &d.to_string());
    }
    if let JsonValue::Object(map) = &ev.attributes {
        for (k, v) in map {
            if let Some(any) = any_value(v) {
                attrs.push(KeyValue {
                    key: k.clone(),
                    value: Some(any),
                    ..Default::default()
                });
            }
        }
        // GenAI semconv mapping: Phoenix synthesizes llm.* / tool.* from these.
        if let Some(model) = map.get("model").and_then(|v| v.as_str()) {
            push_attr(&mut attrs, "gen_ai.request.model", model);
        }
        if let Some(t) = map.get("tokens_in").and_then(|v| v.as_u64()) {
            push_attr(&mut attrs, "gen_ai.usage.input_tokens", &t.to_string());
        }
        if let Some(t) = map.get("tokens_out").and_then(|v| v.as_u64()) {
            push_attr(&mut attrs, "gen_ai.usage.output_tokens", &t.to_string());
        }
        if let Some(t) = map.get("tokens_cached").and_then(|v| v.as_u64()) {
            push_attr(
                &mut attrs,
                "gen_ai.usage.cache_read_input_tokens",
                &t.to_string(),
            );
        }
    }
    if matches!(ev.kind, SpanKind::ToolCall | SpanKind::McpRequest) {
        let tool_name = ev
            .attributes
            .get("tool_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&ev.name);
        push_attr(&mut attrs, "gen_ai.tool.name", tool_name);
    }
    if let Some(err) = &ev.error {
        push_attr(&mut attrs, "error.code", &err.code);
    }
    if let Some(input) = &ev.input {
        if !matches!(input, JsonValue::Null) {
            if let Some(any) = any_value(input) {
                attrs.push(KeyValue {
                    key: "input_summary".into(),
                    value: Some(any),
                    ..Default::default()
                });
            }
            // OpenInference: Phoenix renders input.value (string) for the span input.
            // Structured payloads are JSON-serialized so they stay visible there.
            push_attr(
                &mut attrs,
                "input.value",
                &match input {
                    JsonValue::String(s) => s.clone(),
                    other => other.to_string(),
                },
            );
        }
    }
    if let Some(output) = &ev.output {
        if !matches!(output, JsonValue::Null) {
            if let Some(any) = any_value(output) {
                attrs.push(KeyValue {
                    key: "output_summary".into(),
                    value: Some(any),
                    ..Default::default()
                });
            }
            push_attr(
                &mut attrs,
                "output.value",
                &match output {
                    JsonValue::String(s) => s.clone(),
                    other => other.to_string(),
                },
            );
        }
    }

    let status = if matches!(ev.status, SpanStatus::Error | SpanStatus::Cancelled) {
        let message = ev
            .error
            .as_ref()
            .map(|e| e.message.clone())
            .unwrap_or_default();
        Some(Status {
            message,
            code: 2, // STATUS_CODE_ERROR
        })
    } else {
        Some(Status {
            message: String::new(),
            code: 1, // STATUS_CODE_OK
        })
    };

    Span {
        trace_id: trace_id_bytes(&ev.trace_id).to_vec(),
        span_id: span_id_bytes(&ev.trace_id, &ev.span_id).to_vec(),
        trace_state: String::new(),
        parent_span_id: ev
            .parent_span_id
            .as_ref()
            .map(|p| span_id_bytes(&ev.trace_id, p).to_vec())
            .unwrap_or_default(),
        name: ev.name.clone(),
        kind: otlp_kind(ev.kind),
        start_time_unix_nano: start_ns,
        end_time_unix_nano: end_ns,
        attributes: attrs,
        dropped_attributes_count: 0,
        events: Vec::new(),
        dropped_events_count: 0,
        links: Vec::new(),
        dropped_links_count: 0,
        status,
        flags: 1, // sampled
    }
}

fn push_attr(attrs: &mut Vec<KeyValue>, key: &str, value: &str) {
    attrs.push(KeyValue {
        key: key.to_string(),
        value: Some(AnyValue {
            value: Some(Value::StringValue(value.to_string())),
        }),
        ..Default::default()
    });
}

/// OTel span kind: ToolCall / McpRequest are client calls, everything else is
/// internal (the Run itself is the root).
fn otlp_kind(kind: SpanKind) -> i32 {
    match kind {
        SpanKind::ToolCall | SpanKind::McpRequest => OtlpSpanKind::Client as i32,
        _ => OtlpSpanKind::Internal as i32,
    }
}

/// OpenInference span kind (Phoenix reads `openinference.span.kind` to render
/// LLM / tool / chain details).
fn openinference_kind(kind: SpanKind) -> &'static str {
    match kind {
        SpanKind::LlmCall => "LLM",
        SpanKind::ToolCall | SpanKind::McpRequest => "TOOL",
        SpanKind::Run | SpanKind::AgentLoop => "CHAIN",
        _ => "CHAIN",
    }
}

/// Convert a JSON value into an OTLP `AnyValue`.
fn any_value(v: &JsonValue) -> Option<AnyValue> {
    let value = match v {
        JsonValue::Null => return None,
        JsonValue::Bool(b) => Value::BoolValue(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::IntValue(i)
            } else if let Some(u) = n.as_u64() {
                Value::IntValue(u as i64)
            } else {
                n.as_f64().map(Value::DoubleValue)?
            }
        }
        JsonValue::String(s) => Value::StringValue(s.clone()),
        JsonValue::Array(items) => {
            let values: Vec<AnyValue> = items.iter().filter_map(any_value).collect();
            Value::ArrayValue(ArrayValue { values })
        }
        JsonValue::Object(map) => {
            let values: Vec<KeyValue> = map
                .iter()
                .filter_map(|(k, v)| {
                    any_value(v).map(|val| KeyValue {
                        key: k.clone(),
                        value: Some(val),
                        ..Default::default()
                    })
                })
                .collect();
            Value::KvlistValue(KeyValueList { values })
        }
    };
    Some(AnyValue { value: Some(value) })
}

/// Derive a fixed 16-byte trace id from an arbitrary string id.
fn trace_id_bytes(id: &str) -> [u8; 16] {
    if let Some(bytes) = decode_hex_compact(id) {
        if bytes.len() == 16 {
            return bytes.try_into().expect("16 bytes");
        }
    }
    let digest = Sha256::digest(id.as_bytes());
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest[..16]);
    out
}

/// Derive a fixed 8-byte span id from an arbitrary string id.
///
/// Non-hex ids (e.g. the fixed `"run-root"` label) are only unique within a
/// trace, but Phoenix stores span ids globally unique — so the hash is scoped
/// by `trace_id` to avoid cross-trace collisions (the second run's root span
/// would otherwise be rejected and its children would reparent onto the first
/// trace's run span).
fn span_id_bytes(trace_id: &str, id: &str) -> [u8; 8] {
    if let Some(bytes) = decode_hex_compact(id) {
        if bytes.len() >= 8 {
            let mut out = [0u8; 8];
            out.copy_from_slice(&bytes[..8]);
            return out;
        }
    }
    let digest = Sha256::digest(format!("{trace_id}:{id}").as_bytes());
    let mut out = [0u8; 8];
    out.copy_from_slice(&digest[..8]);
    out
}

/// Hex-decode `id` after stripping `-` separators (uuid form). `None` when
/// the compact form is not an even-length hex string.
fn decode_hex_compact(id: &str) -> Option<Vec<u8>> {
    let compact = id.replace('-', "");
    if !compact.len().is_multiple_of(2) {
        return None;
    }
    let bytes = compact.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        let hi = (chunk[0] as char).to_digit(16)?;
        let lo = (chunk[1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::trace::SpanKind;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn sample_span() -> TraceEvent {
        let mut ev = TraceEvent::new(
            "2f4c6d8e-0000-4000-8000-111111111111",
            "3f5d7e9f-0000-4000-8000-222222222222",
            SpanKind::ToolCall,
            "file_read",
        );
        ev.parent_span_id = Some("run-root-1".into());
        ev.started_at_ms = 1_700_000_000_000;
        ev.end();
        ev
    }

    #[test]
    fn uuid_ids_are_hex_decoded_to_fixed_width() {
        let trace = trace_id_bytes("2f4c6d8e-0000-4000-8000-111111111111");
        assert_eq!(trace.len(), 16);
        assert_eq!(trace[0], 0x2f);
        assert_eq!(trace[15], 0x11);
        let span = span_id_bytes(
            "2f4c6d8e-0000-4000-8000-111111111111",
            "3f5d7e9f-0000-4000-8000-222222222222",
        );
        assert_eq!(span.len(), 8);
        assert_eq!(span[0], 0x3f);
    }

    #[test]
    fn non_hex_ids_use_stable_sha_fallback() {
        let a = trace_id_bytes("run-1");
        let b = trace_id_bytes("run-1");
        assert_eq!(a.len(), 16);
        assert_eq!(a, b);
        let c = trace_id_bytes("run-2");
        assert_ne!(a, c);
        let sa = span_id_bytes("run-1", "span-x");
        assert_eq!(sa.len(), 8);
        assert_eq!(sa, span_id_bytes("run-1", "span-x"));
        assert_ne!(sa, span_id_bytes("run-1", "span-y"));
    }

    #[test]
    fn non_hex_span_ids_are_scoped_by_trace() {
        // Regression: the fixed "run-root" label must not collide across
        // traces (Phoenix enforces a global span_id uniqueness, so the second
        // run's root span was rejected and its children reparented onto the
        // first trace's run span).
        let a = span_id_bytes("trace-a", "run-root");
        let b = span_id_bytes("trace-b", "run-root");
        assert_ne!(a, b);
        // Stable within the same trace (parent/child consistency).
        assert_eq!(a, span_id_bytes("trace-a", "run-root"));
    }

    fn decode_req(bytes: Vec<u8>) -> ExportTraceServiceRequest {
        ExportTraceServiceRequest::decode(&bytes[..]).expect("decode protobuf")
    }

    fn first_span(req: &ExportTraceServiceRequest) -> &Span {
        &req.resource_spans[0].scope_spans[0].spans[0]
    }

    fn attr_str(span: &Span, key: &str) -> Option<String> {
        span.attributes.iter().find(|a| a.key == key).and_then(|a| {
            a.value.as_ref().and_then(|v| match &v.value {
                Some(Value::StringValue(s)) => Some(s.clone()),
                _ => None,
            })
        })
    }

    #[test]
    fn build_export_request_shape() {
        let req = decode_req(build_export_request(&[sample_span()], "pointer-app"));
        let service = &req.resource_spans[0].resource.as_ref().unwrap().attributes[0];
        assert_eq!(service.key, "service.name");
        let span = first_span(&req);
        assert_eq!(span.name, "file_read");
        assert_eq!(span.kind, OtlpSpanKind::Client as i32); // CLIENT for ToolCall
        assert_eq!(span.status.as_ref().unwrap().code, 1);
        assert!(!span.parent_span_id.is_empty());
        assert_eq!(span.trace_id.len(), 16);
        assert_eq!(span.start_time_unix_nano, 1_700_000_000_000_000_000);
        let keys: Vec<&str> = span.attributes.iter().map(|a| a.key.as_str()).collect();
        assert!(keys.contains(&"openinference.span.kind"));
        assert!(keys.contains(&"span.kind"));
        assert!(keys.contains(&"run_id"));
        assert!(keys.contains(&"duration_ms"));
        assert!(keys.contains(&"gen_ai.tool.name"));
    }

    #[test]
    fn error_span_maps_status_and_message() {
        let mut ev = sample_span();
        ev.set_error("mcp_call_failed", "MCP tools/call 失败");
        let req = decode_req(build_export_request(&[ev], "pointer-app"));
        let span = first_span(&req);
        let status = span.status.as_ref().unwrap();
        assert_eq!(status.code, 2);
        assert_eq!(status.message, "MCP tools/call 失败");
        assert!(span.attributes.iter().any(|a| a.key == "error.code"));
    }

    #[test]
    fn openinference_kind_mapping() {
        assert_eq!(openinference_kind(SpanKind::LlmCall), "LLM");
        assert_eq!(openinference_kind(SpanKind::ToolCall), "TOOL");
        assert_eq!(openinference_kind(SpanKind::McpRequest), "TOOL");
        assert_eq!(openinference_kind(SpanKind::Run), "CHAIN");
        assert_eq!(openinference_kind(SpanKind::AgentLoop), "CHAIN");
        assert_eq!(openinference_kind(SpanKind::Hook), "CHAIN");
    }

    #[test]
    fn llm_span_maps_gen_ai_attributes() {
        let mut ev = TraceEvent::new("r1", "s1", SpanKind::LlmCall, "lead");
        ev.conversation_id = "conv-1".into();
        ev.attributes = serde_json::json!({
            "model": "deepseek-v4-flash",
            "tokens_in": 100,
            "tokens_out": 20,
            "tokens_cached": 80,
        });
        ev.end();
        let req = decode_req(build_export_request(&[ev], "pointer-app"));
        let span = first_span(&req);
        assert_eq!(
            attr_str(span, "openinference.span.kind").as_deref(),
            Some("LLM")
        );
        assert_eq!(
            attr_str(span, "gen_ai.request.model").as_deref(),
            Some("deepseek-v4-flash")
        );
        assert_eq!(
            attr_str(span, "gen_ai.usage.input_tokens").as_deref(),
            Some("100")
        );
        assert_eq!(
            attr_str(span, "gen_ai.usage.output_tokens").as_deref(),
            Some("20")
        );
        assert_eq!(
            attr_str(span, "gen_ai.usage.cache_read_input_tokens").as_deref(),
            Some("80")
        );
        assert_eq!(
            attr_str(span, "gen_ai.conversation.id").as_deref(),
            Some("conv-1")
        );
    }

    #[test]
    fn input_output_payloads_map_to_value_attrs() {
        let mut ev = TraceEvent::new("r1", "s1", SpanKind::ToolCall, "file_list");
        ev.input = Some(serde_json::json!({ "path": "src", "recursive": true }));
        ev.output = Some(serde_json::json!("ok"));
        ev.end();
        let req = decode_req(build_export_request(&[ev], "pointer-app"));
        let span = first_span(&req);
        // Structured input is JSON-serialized into input.value (OpenInference).
        let input = attr_str(span, "input.value").expect("input.value");
        let parsed: serde_json::Value = serde_json::from_str(&input).expect("json");
        assert_eq!(parsed["path"], "src");
        assert_eq!(parsed["recursive"], true);
        // String output passes through verbatim.
        assert_eq!(attr_str(span, "output.value").as_deref(), Some("ok"));
        // Structured summary attrs are still present.
        assert!(span.attributes.iter().any(|a| a.key == "input_summary"));
        assert!(span.attributes.iter().any(|a| a.key == "output_summary"));

        // Null payloads emit no value attr.
        let mut ev2 = TraceEvent::new("r1", "s2", SpanKind::ToolCall, "noop");
        ev2.input = Some(serde_json::Value::Null);
        ev2.end();
        let req2 = decode_req(build_export_request(&[ev2], "pointer-app"));
        assert!(attr_str(first_span(&req2), "input.value").is_none());
    }

    #[test]
    fn any_value_recursive_shapes() {
        let v = serde_json::json!({
            "nested": { "ok": true, "n": 3 },
            "list": ["a", 1],
        });
        let any = any_value(&v).unwrap();
        let Value::KvlistValue(kv) = any.value.unwrap() else {
            panic!("expected kvlist");
        };
        let nested = kv
            .values
            .iter()
            .find(|e| e.key == "nested")
            .expect("nested");
        let Value::KvlistValue(nested_kv) = nested.value.as_ref().unwrap().value.as_ref().unwrap()
        else {
            panic!("expected nested kvlist");
        };
        let ok = nested_kv.values.iter().find(|e| e.key == "ok").expect("ok");
        assert!(matches!(
            ok.value.as_ref().unwrap().value,
            Some(Value::BoolValue(true))
        ));
        let list = kv.values.iter().find(|e| e.key == "list").expect("list");
        let Value::ArrayValue(arr) = list.value.as_ref().unwrap().value.as_ref().unwrap() else {
            panic!("expected array");
        };
        assert!(matches!(arr.values[1].value, Some(Value::IntValue(1))));
    }

    #[tokio::test]
    async fn export_posts_protobuf_to_collector() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 16 * 1024];
            let mut raw = Vec::new();
            loop {
                let n = sock.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
                if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header_end = pos + 4;
                    let headers = String::from_utf8_lossy(&raw[..header_end]).to_string();
                    let content_length: usize = headers
                        .lines()
                        .find_map(|l| {
                            let lower = l.to_ascii_lowercase();
                            lower
                                .starts_with("content-length:")
                                .then(|| l.split(':').nth(1)?.trim().parse().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    while raw.len() < header_end + content_length {
                        let n = sock.read(&mut buf).await.unwrap();
                        if n == 0 {
                            break;
                        }
                        raw.extend_from_slice(&buf[..n]);
                    }
                    let body = raw[header_end..header_end + content_length].to_vec();
                    let _ = sock
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
                        .await;
                    return (headers, body);
                }
            }
            (String::new(), Vec::new())
        });

        let exporter = OtlpExporter::new(format!("http://{addr}/v1/traces"), "pointer-app");
        exporter
            .export(vec![sample_span()])
            .await
            .expect("export ok");

        let (headers, body) = server.await.unwrap();
        assert!(headers.starts_with("POST /v1/traces"), "{headers}");
        assert!(headers
            .to_ascii_lowercase()
            .contains("content-type: application/x-protobuf"));
        let req = ExportTraceServiceRequest::decode(&body[..]).expect("protobuf body");
        let span = &req.resource_spans[0].scope_spans[0].spans[0];
        assert_eq!(span.name, "file_read");
    }

    #[tokio::test]
    async fn unreachable_endpoint_returns_err_not_panic() {
        let exporter = OtlpExporter::new("http://127.0.0.1:1/v1/traces", "pointer-app");
        let err = exporter.export(vec![sample_span()]).await;
        assert!(err.is_err());
    }

    #[test]
    fn from_env_respects_protocol_and_endpoints() {
        // No config -> None.
        let empty: [(String, String); 0] = [];
        let lookup = |key: &str| empty.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        assert!(OtlpExporter::from_env_impl(lookup).is_none());

        // Base endpoint -> /v1/traces appended.
        let env = [(
            "OTEL_EXPORTER_OTLP_ENDPOINT".to_string(),
            "http://collector:4318".to_string(),
        )];
        let lookup = |key: &str| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        let e = OtlpExporter::from_env_impl(lookup).expect("base endpoint");
        assert_eq!(e.endpoint, "http://collector:4318/v1/traces");

        // Full traces endpoint wins.
        let env = [
            (
                "OTEL_EXPORTER_OTLP_ENDPOINT".to_string(),
                "http://collector:4318".to_string(),
            ),
            (
                "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT".to_string(),
                "http://c:4318/custom".to_string(),
            ),
        ];
        let lookup = |key: &str| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        let e = OtlpExporter::from_env_impl(lookup).expect("traces endpoint");
        assert_eq!(e.endpoint, "http://c:4318/custom");

        // Unsupported protocol -> None.
        let env = [
            (
                "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT".to_string(),
                "http://c:4318/custom".to_string(),
            ),
            (
                "OTEL_EXPORTER_OTLP_PROTOCOL".to_string(),
                "grpc".to_string(),
            ),
        ];
        let lookup = |key: &str| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        assert!(OtlpExporter::from_env_impl(lookup).is_none());

        // http/protobuf accepted.
        let env = [
            (
                "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT".to_string(),
                "http://c:4318/custom".to_string(),
            ),
            (
                "OTEL_EXPORTER_OTLP_PROTOCOL".to_string(),
                "http/protobuf".to_string(),
            ),
        ];
        let lookup = |key: &str| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        assert!(OtlpExporter::from_env_impl(lookup).is_some());

        // http/json rejected (Phoenix only accepts protobuf).
        let env = [
            (
                "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT".to_string(),
                "http://c:4318/custom".to_string(),
            ),
            (
                "OTEL_EXPORTER_OTLP_PROTOCOL".to_string(),
                "http/json".to_string(),
            ),
        ];
        let lookup = |key: &str| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        assert!(OtlpExporter::from_env_impl(lookup).is_none());
    }
}
