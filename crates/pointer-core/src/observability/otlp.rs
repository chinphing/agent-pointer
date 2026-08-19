//! OTLP exporter (plan §7.3, P4 ③).
//!
//! Exports [`TraceEvent`] batches to an OpenTelemetry Collector via the
//! **OTLP/HTTP + JSON** protocol (`POST {endpoint}/v1/traces`,
//! `Content-Type: application/json`). JSON encoding is chosen over
//! protobuf/gRPC so no prost/tonic dependencies are needed; the OTLP spec
//! mandates JSON as a first-class encoding and the Collector accepts it.
//!
//! Activation follows the standard OpenTelemetry environment variables:
//! - `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` — full `.../v1/traces` endpoint
//! - `OTEL_EXPORTER_OTLP_ENDPOINT` — base endpoint (signal path appended)
//! - `OTEL_EXPORTER_OTLP_PROTOCOL` — only `http/json` (default) is supported;
//!   `grpc` / `http/protobuf` are logged and skipped
//! - `OTEL_SERVICE_NAME` — default `pointer-app`
//!
//! Export failures are fail-open: the pipeline logs them and continues
//! (fault isolation is handled by [`ExporterRegistry::export_batch`]).
//!
//! Trace/span ids in Pointer are strings (`run_id` for trace, uuid for span).
//! OTLP requires fixed-width byte ids: hex-decodable strings (uuid form) are
//! decoded, anything else falls back to a stable sha256 derivation.

use std::env;

use async_trait::async_trait;
use base64::Engine as _;
use serde_json::{json, Value};

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
            if !proto.is_empty() && proto != "http/json" {
                log::warn!(
                    "OtlpExporter: unsupported OTEL_EXPORTER_OTLP_PROTOCOL={proto} \
                     (only http/json); skipping OTLP export"
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
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("otlp http request failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("otlp http status {}", resp.status()));
        }
        Ok(())
    }
}

/// Serialize a batch into an `ExportTraceServiceRequest` JSON body
/// (OTLP/HTTP + JSON encoding).
pub fn build_export_request(batch: &[TraceEvent], service_name: &str) -> Value {
    let spans: Vec<Value> = batch.iter().map(span_to_json).collect();
    json!({
        "resourceSpans": [{
            "resource": {
                "attributes": [
                    { "key": "service.name", "value": { "stringValue": service_name } }
                ]
            },
            "scopeSpans": [{
                "scope": { "name": "pointer-core", "version": env!("CARGO_PKG_VERSION") },
                "spans": spans
            }]
        }]
    })
}

fn span_to_json(ev: &TraceEvent) -> Value {
    let b64 = base64::engine::general_purpose::STANDARD;
    let trace_id = b64.encode(trace_id_bytes(&ev.trace_id));
    let span_id = b64.encode(span_id_bytes(&ev.span_id));

    let start_ns = (ev.started_at_ms.max(0) as u64).saturating_mul(1_000_000);
    let end_ms = ev.ended_at_ms.unwrap_or(ev.started_at_ms);
    let end_ns = (end_ms.max(0) as u64).saturating_mul(1_000_000);

    let mut attrs = Vec::new();
    push_attr(&mut attrs, "span.kind", json!({ "stringValue": ev.kind.as_str() }));
    push_attr(&mut attrs, "run_id", json!({ "stringValue": ev.run_id }));
    push_attr(
        &mut attrs,
        "conversation_id",
        json!({ "stringValue": ev.conversation_id }),
    );
    if let Some(d) = ev.duration_ms {
        push_attr(&mut attrs, "duration_ms", json!({ "intValue": d.to_string() }));
    }
    if let Value::Object(map) = &ev.attributes {
        for (k, v) in map {
            if let Some(any) = any_value(v) {
                push_attr(&mut attrs, k, any);
            }
        }
    }
    if let Some(err) = &ev.error {
        push_attr(
            &mut attrs,
            "error.code",
            json!({ "stringValue": err.code }),
        );
    }
    if let Some(input) = &ev.input {
        if let Some(any) = any_value(input) {
            push_attr(&mut attrs, "input_summary", any);
        }
    }
    if let Some(output) = &ev.output {
        if let Some(any) = any_value(output) {
            push_attr(&mut attrs, "output_summary", any);
        }
    }

    let mut status = json!({ "code": 1 }); // STATUS_CODE_OK
    if matches!(ev.status, SpanStatus::Error | SpanStatus::Cancelled) {
        status = json!({ "code": 2 }); // STATUS_CODE_ERROR
        if let Some(err) = &ev.error {
            status["message"] = json!(err.message);
        }
    }

    let mut span = json!({
        "traceId": trace_id,
        "spanId": span_id,
        "name": ev.name,
        "kind": otlp_kind(ev.kind),
        "startTimeUnixNano": start_ns.to_string(),
        "endTimeUnixNano": end_ns.to_string(),
        "attributes": attrs,
        "status": status,
    });
    if let Some(parent) = &ev.parent_span_id {
        span["parentSpanId"] = json!(b64.encode(span_id_bytes(parent)));
    }
    span
}

fn push_attr(attrs: &mut Vec<Value>, key: &str, value: Value) {
    attrs.push(json!({ "key": key, "value": value }));
}

/// OTel span kind: ToolCall / McpRequest are client calls, everything else is
/// internal (the Run itself is the root).
fn otlp_kind(kind: SpanKind) -> i32 {
    match kind {
        SpanKind::ToolCall | SpanKind::McpRequest => 3, // CLIENT
        _ => 1,                                         // INTERNAL
    }
}

/// Convert a JSON value into an OTLP `AnyValue`.
fn any_value(v: &Value) -> Option<Value> {
    match v {
        Value::Null => None,
        Value::Bool(b) => Some(json!({ "boolValue": b })),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(json!({ "intValue": i.to_string() }))
            } else if let Some(u) = n.as_u64() {
                Some(json!({ "intValue": u.to_string() }))
            } else {
                n.as_f64().map(|f| json!({ "doubleValue": f }))
            }
        }
        Value::String(s) => Some(json!({ "stringValue": s })),
        Value::Array(items) => {
            let values: Vec<Value> = items.iter().filter_map(any_value).collect();
            Some(json!({ "arrayValue": { "values": values } }))
        }
        Value::Object(map) => {
            let values: Vec<Value> = map
                .iter()
                .filter_map(|(k, v)| any_value(v).map(|val| json!({ "key": k, "value": val })))
                .collect();
            Some(json!({ "kvlistValue": { "values": values } }))
        }
    }
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
fn span_id_bytes(id: &str) -> [u8; 8] {
    if let Some(bytes) = decode_hex_compact(id) {
        if bytes.len() >= 8 {
            let mut out = [0u8; 8];
            out.copy_from_slice(&bytes[..8]);
            return out;
        }
    }
    let digest = Sha256::digest(id.as_bytes());
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
        let span = span_id_bytes("3f5d7e9f-0000-4000-8000-222222222222");
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
        let sa = span_id_bytes("span-x");
        assert_eq!(sa.len(), 8);
        assert_eq!(sa, span_id_bytes("span-x"));
        assert_ne!(sa, span_id_bytes("span-y"));
    }

    #[test]
    fn build_export_request_shape() {
        let req = build_export_request(&[sample_span()], "pointer-app");
        let span = &req["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
        assert_eq!(req["resourceSpans"][0]["resource"]["attributes"][0]["key"], "service.name");
        assert_eq!(span["name"], "file_read");
        assert_eq!(span["kind"], 3); // CLIENT for ToolCall
        assert_eq!(span["status"]["code"], 1);
        assert!(span.get("parentSpanId").is_some());
        assert!(span["traceId"].as_str().unwrap().len() >= 22); // base64(16)
        assert_eq!(span["startTimeUnixNano"].as_str().unwrap(), "1700000000000000000");
        let attrs = span["attributes"].as_array().unwrap();
        let keys: Vec<&str> = attrs.iter().map(|a| a["key"].as_str().unwrap()).collect();
        assert!(keys.contains(&"span.kind"));
        assert!(keys.contains(&"run_id"));
        assert!(keys.contains(&"duration_ms"));
    }

    #[test]
    fn error_span_maps_status_and_message() {
        let mut ev = sample_span();
        ev.set_error("mcp_call_failed", "MCP tools/call 失败");
        let span = &build_export_request(&[ev], "pointer-app")["resourceSpans"][0]
            ["scopeSpans"][0]["spans"][0];
        assert_eq!(span["status"]["code"], 2);
        assert_eq!(span["status"]["message"], "MCP tools/call 失败");
        let attrs = span["attributes"].as_array().unwrap();
        assert!(attrs.iter().any(|a| a["key"] == "error.code" && a["value"]["stringValue"] == "mcp_call_failed"));
    }

    #[test]
    fn any_value_recursive_shapes() {
        let v = json!({
            "nested": { "ok": true, "n": 3 },
            "list": ["a", 1],
        });
        let any = any_value(&v).unwrap();
        let values = any["kvlistValue"]["values"].as_array().unwrap();
        let nested = values.iter().find(|e| e["key"] == "nested").expect("nested");
        let nested_kv = nested["value"]["kvlistValue"]["values"]
            .as_array()
            .expect("nested kv");
        let ok = nested_kv.iter().find(|e| e["key"] == "ok").expect("ok");
        assert_eq!(ok["value"]["boolValue"], true);
        let list = values.iter().find(|e| e["key"] == "list").expect("list");
        assert_eq!(list["value"]["arrayValue"]["values"][1]["intValue"], "1");
    }

    #[tokio::test]
    async fn export_posts_json_to_collector() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 16 * 1024];
            let n = sock.read(&mut buf).await.unwrap();
            let text = String::from_utf8_lossy(&buf[..n]).to_string();
            let header_end = text.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
            let headers = &text[..header_end];
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
            let mut body = text[header_end..].to_string();
            while body.len() < content_length {
                let n = sock.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                body.push_str(&String::from_utf8_lossy(&buf[..n]));
            }
            let _ = sock
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
                .await;
            (text, body)
        });

        let exporter = OtlpExporter::new(format!("http://{addr}/v1/traces"), "pointer-app");
        exporter.export(vec![sample_span()]).await.expect("export ok");

        let (text, body) = server.await.unwrap();
        assert!(text.starts_with("POST /v1/traces"), "{text}");
        assert!(text.to_ascii_lowercase().contains("content-type: application/json"));
        let req: Value = serde_json::from_str(&body).expect("json body");
        assert_eq!(req["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["name"], "file_read");
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
        let lookup = |key: &str| {
            empty
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };
        assert!(OtlpExporter::from_env_impl(lookup).is_none());

        // Base endpoint -> /v1/traces appended.
        let env = [(
            "OTEL_EXPORTER_OTLP_ENDPOINT".to_string(),
            "http://collector:4318".to_string(),
        )];
        let lookup = |key: &str| {
            env.iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };
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
        let lookup = |key: &str| {
            env.iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };
        let e = OtlpExporter::from_env_impl(lookup).expect("traces endpoint");
        assert_eq!(e.endpoint, "http://c:4318/custom");

        // Unsupported protocol -> None.
        let env = [
            (
                "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT".to_string(),
                "http://c:4318/custom".to_string(),
            ),
            ("OTEL_EXPORTER_OTLP_PROTOCOL".to_string(), "grpc".to_string()),
        ];
        let lookup = |key: &str| {
            env.iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };
        assert!(OtlpExporter::from_env_impl(lookup).is_none());

        // http/json accepted.
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
        let lookup = |key: &str| {
            env.iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };
        assert!(OtlpExporter::from_env_impl(lookup).is_some());
    }
}
