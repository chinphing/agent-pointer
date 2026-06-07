//! JSON tool-call protocol: one JSON object per assistant turn (`response_format: json_object`).
//! Streaming partial fields use [`partial_json_fixer::fix_json`] (partial JSON repair) plus serde.
//! Final parsing also tries [`repair_json_unescaped_quotes`] and finalize-only
//! [`crate::json_interior_quote_escape::munge_finalize_json_parse`] (literal newlines/tabs in
//! strings, then quote repair) when strict JSON and `fix_json` are not enough (see
//! [`parse_tool_json_value`]).

use std::borrow::Cow;
use std::collections::HashMap;

use partial_json_fixer::fix_json;
use serde_json::Value;

use crate::json_interior_quote_escape::{munge_finalize_json_parse, repair_json_unescaped_quotes};
use crate::tool_envelope::{ToolEnvelope, ToolEnvelopeCall};

/// Diagnostics after the stream ends (JSON tool protocol).
#[derive(Debug, Clone, Default)]
pub struct JsonToolFinishDiagnostics {
    pub attempted_tool_json: bool,
    pub fragment_complete: bool,
    pub parse_error: Option<String>,
    pub vacuous_fragments_skipped: u32,
    pub feed_lane_tail: String,
    pub consumed_fragment_chars: Option<usize>,
    pub consumed_fragment_head: Option<String>,
    pub parser_buffer_remaining_chars: usize,
    pub merge_ui_order_reparse_ok: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonFeedLane {
    Content,
    Reasoning,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JsonStreamingPartial {
    pub thoughts: Option<String>,
    pub headline: Option<String>,
    pub tool_name: Option<String>,
    /// `response` tool: `tool_args.text` (user-visible reply), as repaired so far.
    pub response_text: Option<String>,
}

/// Tracks stream provenance for diagnostics (mirrors XML parser counters).
pub struct JsonToolParser {
    pub(crate) ingest_content_chunks: u32,
    pub(crate) ingest_reasoning_chunks: u32,
    pub(crate) ingest_content_chars: usize,
    pub(crate) ingest_reasoning_chars: usize,
    pub(crate) feed_lane_tail: String,
}

impl JsonToolParser {
    pub fn new() -> Self {
        Self {
            ingest_content_chunks: 0,
            ingest_reasoning_chunks: 0,
            ingest_content_chars: 0,
            ingest_reasoning_chars: 0,
            feed_lane_tail: String::new(),
        }
    }

    pub fn feed_lane(&mut self, chunk: &str, lane: JsonFeedLane) {
        if chunk.is_empty() {
            return;
        }
        match lane {
            JsonFeedLane::Content => {
                self.ingest_content_chunks += 1;
                self.ingest_content_chars += chunk.chars().count();
            }
            JsonFeedLane::Reasoning => {
                self.ingest_reasoning_chunks += 1;
                self.ingest_reasoning_chars += chunk.chars().count();
            }
        }
        self.push_lane_tail(lane);
    }

    fn push_lane_tail(&mut self, lane: JsonFeedLane) {
        self.feed_lane_tail.push(match lane {
            JsonFeedLane::Content => 'C',
            JsonFeedLane::Reasoning => 'R',
        });
        const MAX: usize = 96;
        if self.feed_lane_tail.len() > MAX {
            let excess = self.feed_lane_tail.len() - MAX;
            self.feed_lane_tail.drain(..excess);
        }
    }

}

fn compact_fragment_head(s: &str, max_chars: usize) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    let t = t.trim();
    let count = t.chars().count();
    if count <= max_chars {
        t.to_string()
    } else {
        format!(
            "{}…(+{} chars)",
            t.chars().take(max_chars).collect::<String>(),
            count.saturating_sub(max_chars)
        )
    }
}

fn strip_markdown_json_fence(s: &str) -> Cow<'_, str> {
    let t = s.trim();
    if !t.starts_with("```") {
        return Cow::Borrowed(t);
    }
    let mut rest = t.strip_prefix("```").unwrap_or(t);
    rest = rest.trim_start();
    if rest.starts_with("json") {
        rest = rest["json".len()..].trim_start();
    }
    if let Some(end) = rest.rfind("```") {
        Cow::Owned(rest[..end].trim().to_string())
    } else {
        Cow::Borrowed(t)
    }
}

fn value_to_arg_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn json_args_to_string_map(args: &Value) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if let Some(obj) = args.as_object() {
        for (k, v) in obj {
            m.insert(k.clone(), value_to_arg_string(v));
        }
    }
    m
}

fn is_vacuous_tool_json(env: &ToolEnvelope) -> bool {
    env.sidecar.is_empty()
        && env.primary.name.trim().is_empty()
        && env.primary.thoughts.trim().is_empty()
        && env.primary.headline.trim().is_empty()
        && env.primary.arguments.is_empty()
}

/// Try strict parse, then [`fix_json`], then [`repair_json_unescaped_quotes`] on the raw and fixed
/// buffers, then finalize-only [`munge_finalize_json_parse`] (literal newlines/tabs in strings + quote repair).
fn parse_tool_json_value(work_str: &str) -> Result<Value, String> {
    if let Ok(v) = serde_json::from_str::<Value>(work_str) {
        return Ok(v);
    }

    let fixed = fix_json(work_str);
    if let Ok(v) = serde_json::from_str::<Value>(&fixed) {
        if fixed != work_str {
            log::debug!(
                "json_tool_caller: parsed tool JSON after partial-json-fixer (chars {} -> {})",
                work_str.chars().count(),
                fixed.chars().count()
            );
        }
        return Ok(v);
    }

    let rq_work = repair_json_unescaped_quotes(work_str);
    if rq_work != work_str {
        if let Ok(v) = serde_json::from_str::<Value>(&rq_work) {
            log::warn!(
                "json_tool_caller: parsed tool JSON after repair_json_unescaped_quotes on raw buffer (chars {})",
                work_str.chars().count()
            );
            return Ok(v);
        }
    }

    let rq_fixed = repair_json_unescaped_quotes(&fixed);
    if rq_fixed != fixed && rq_fixed != rq_work {
        if let Ok(v) = serde_json::from_str::<Value>(&rq_fixed) {
            log::warn!(
                "json_tool_caller: parsed tool JSON after repair_json_unescaped_quotes on partial-json-fixer output (chars {})",
                fixed.chars().count()
            );
            return Ok(v);
        }
    }

    let munge_work = munge_finalize_json_parse(work_str);
    if munge_work != work_str && munge_work != rq_work && munge_work != rq_fixed {
        if let Ok(v) = serde_json::from_str::<Value>(&munge_work) {
            log::warn!(
                "json_tool_caller: parsed tool JSON after finalize munge (newline/tab escape + quote repair) on raw buffer (chars {})",
                work_str.chars().count()
            );
            return Ok(v);
        }
    }

    let munge_fixed = munge_finalize_json_parse(&fixed);
    if munge_fixed != fixed && munge_fixed != rq_fixed && munge_fixed != munge_work {
        if let Ok(v) = serde_json::from_str::<Value>(&munge_fixed) {
            log::warn!(
                "json_tool_caller: parsed tool JSON after finalize munge on partial-json-fixer output (chars {})",
                fixed.chars().count()
            );
            return Ok(v);
        }
    }

    serde_json::from_str::<Value>(&munge_fixed)
        .or_else(|_| serde_json::from_str::<Value>(&munge_work))
        .or_else(|_| serde_json::from_str::<Value>(&rq_fixed))
        .or_else(|_| serde_json::from_str::<Value>(&rq_work))
        .or_else(|_| serde_json::from_str::<Value>(&fixed))
        .map_err(|e| e.to_string())
}

/// Parse the final assistant `content` buffer into the same envelope shape as the legacy XML path.
pub fn finalize_json_tool_envelope(
    content_buf: &str,
    reasoning_buf: &str,
) -> (Option<ToolEnvelope>, JsonToolFinishDiagnostics) {
    let mut diag = JsonToolFinishDiagnostics::default();
    let attempted_tool_json = content_buf.contains("\"tool_name\"")
        || content_buf.contains("\"tool_args\"")
        || reasoning_buf.contains("\"tool_name\"")
        || reasoning_buf.contains("\"tool_args\"");
    diag.attempted_tool_json = attempted_tool_json;

    let work = strip_markdown_json_fence(content_buf.trim());
    let work_str = match &work {
        Cow::Borrowed(b) => *b,
        Cow::Owned(o) => o.as_str(),
    };

    diag.parser_buffer_remaining_chars = work_str.len();

    if work_str.is_empty() {
        return (None, diag);
    }

    let parsed = parse_tool_json_value(work_str);

    match parsed {
        Ok(v) => match envelope_from_value(&v) {
            Ok(env) if is_vacuous_tool_json(&env) => {
                diag.fragment_complete = true;
                diag.vacuous_fragments_skipped = 1;
                diag.consumed_fragment_chars = Some(work_str.chars().count());
                diag.consumed_fragment_head = Some(compact_fragment_head(work_str, 260));
                (None, diag)
            }
            Ok(env) => {
                diag.fragment_complete = true;
                diag.consumed_fragment_chars = Some(work_str.chars().count());
                diag.consumed_fragment_head = Some(compact_fragment_head(work_str, 260));
                (Some(env), diag)
            }
            Err(e) => {
                diag.fragment_complete = true;
                diag.parse_error = Some(e);
                diag.consumed_fragment_chars = Some(work_str.chars().count());
                diag.consumed_fragment_head = Some(compact_fragment_head(work_str, 260));
                (None, diag)
            }
        },
        Err(e) => {
            diag.parse_error = Some(e.to_string());
            (None, diag)
        }
    }
}

fn envelope_from_value(v: &Value) -> Result<ToolEnvelope, String> {
    let obj = v.as_object().ok_or("root must be a JSON object")?;
    let thoughts = obj
        .get("thoughts")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let headline = obj
        .get("headline")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();

    let mut sidecar: Vec<ToolEnvelopeCall> = Vec::new();
    if let Some(Value::Array(arr)) = obj.get("sidecar_tools") {
        for (i, item) in arr.iter().enumerate() {
            let o = item
                .as_object()
                .ok_or_else(|| format!("sidecar_tools[{i}] must be an object"))?;
            let name = o
                .get("tool_name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let args = o.get("tool_args").cloned().unwrap_or(Value::Object(Default::default()));
            sidecar.push(ToolEnvelopeCall {
                name,
                arguments: json_args_to_string_map(&args),
                thoughts: String::new(),
                headline: String::new(),
            });
        }
    }

    let primary_name = obj
        .get("tool_name")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let primary_args = obj
        .get("tool_args")
        .cloned()
        .unwrap_or(Value::Object(Default::default()));
    let primary = ToolEnvelopeCall {
        name: primary_name,
        arguments: json_args_to_string_map(&primary_args),
        thoughts,
        headline,
    };

    Ok(ToolEnvelope { sidecar, primary })
}

fn extract_tool_args_text(obj: &serde_json::Map<String, Value>) -> Option<String> {
    obj.get("tool_args")
        .and_then(|a| a.as_object())
        .and_then(|m| m.get("text"))
        .and_then(|t| match t {
            Value::String(s) => Some(s.clone()).filter(|s| !s.is_empty()),
            _ => None,
        })
}

fn parse_value_for_streaming_partial(work_str: &str) -> Option<Value> {
    let fixed = fix_json(work_str);
    if let Ok(v) = serde_json::from_str::<Value>(&fixed) {
        return Some(v);
    }
    let rq_fixed = repair_json_unescaped_quotes(&fixed);
    if rq_fixed != fixed {
        if let Ok(v) = serde_json::from_str::<Value>(&rq_fixed) {
            return Some(v);
        }
    }
    let rq_raw = repair_json_unescaped_quotes(work_str);
    if rq_raw != work_str && rq_raw != rq_fixed && rq_raw != fixed {
        return serde_json::from_str(&rq_raw).ok();
    }
    None
}

/// Progressive UI: [`fix_json`], optional [`repair_json_unescaped_quotes`], then read string fields.
pub fn extract_json_streaming_partial(buf: &str) -> JsonStreamingPartial {
    let trimmed = buf.trim();
    if trimmed.is_empty() {
        return JsonStreamingPartial::default();
    }
    let work = strip_markdown_json_fence(trimmed);
    let work_str = match &work {
        Cow::Borrowed(b) => *b,
        Cow::Owned(o) => o.as_str(),
    };
    let Some(v) = parse_value_for_streaming_partial(work_str) else {
        return JsonStreamingPartial::default();
    };
    let Some(obj) = v.as_object() else {
        return JsonStreamingPartial::default();
    };
    let tool_name = obj
        .get("tool_name")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let response_text = match tool_name.as_deref() {
        Some("response") => extract_tool_args_text(obj),
        None => extract_tool_args_text(obj),
        _ => None,
    };
    JsonStreamingPartial {
        thoughts: obj
            .get("thoughts")
            .and_then(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        headline: obj
            .get("headline")
            .and_then(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        tool_name,
        response_text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalize_parses_response_tool() {
        let j = r#"{"thoughts":"t","headline":"h","tool_name":"response","tool_args":{"text":"Hello"}}"#;
        let (env, diag) = finalize_json_tool_envelope(j, "");
        assert!(diag.parse_error.is_none(), "{diag:?}");
        let env = env.expect("envelope");
        assert_eq!(env.primary.name, "response");
        assert_eq!(env.primary.thoughts, "t");
        assert_eq!(env.primary.arguments.get("text").map(String::as_str), Some("Hello"));
    }

    #[test]
    fn finalize_repairs_unescaped_quotes_in_response_text() {
        // `r##` so the closing `"` before `}}` is not swallowed by `r#"…"#`.
        let bad = r##"{"thoughts":"","headline":"","tool_name":"response","tool_args":{"text":"a"b"}}"##;
        let (env, diag) = finalize_json_tool_envelope(bad, "");
        assert!(diag.parse_error.is_none(), "{diag:?}");
        let env = env.expect("envelope");
        assert_eq!(env.primary.name, "response");
        assert_eq!(env.primary.arguments.get("text").map(String::as_str), Some("a\"b"));
    }

    #[test]
    fn finalize_repairs_literal_newline_in_response_text() {
        let bad = concat!(
            r##"{"thoughts":"","headline":"","tool_name":"response","tool_args":{"text":"line1"##,
            "\n",
            r##"line2"}}"##
        );
        assert!(
            serde_json::from_str::<serde_json::Value>(bad).is_err(),
            "fixture must be invalid JSON (literal newline in string)"
        );
        let (env, diag) = finalize_json_tool_envelope(bad, "");
        assert!(diag.parse_error.is_none(), "{:?}", diag.parse_error);
        let env = env.expect("envelope");
        assert_eq!(env.primary.name, "response");
        assert_eq!(
            env.primary.arguments.get("text").map(String::as_str),
            Some("line1\nline2")
        );
    }

    #[test]
    fn partial_streaming_extracts_fields() {
        let partial = r#"{"thoughts":"a","headline":"b","tool_name":"wait","tool_args":{"seconds":"#;
        let p = extract_json_streaming_partial(partial);
        assert_eq!(p.thoughts.as_deref(), Some("a"));
        assert_eq!(p.headline.as_deref(), Some("b"));
        assert_eq!(p.tool_name.as_deref(), Some("wait"));
    }

    #[test]
    fn partial_streaming_extracts_response_text() {
        let partial = r#"{"tool_name":"response","tool_args":{"text":"Hello wor"#;
        let p = extract_json_streaming_partial(partial);
        assert_eq!(p.tool_name.as_deref(), Some("response"));
        assert_eq!(p.response_text.as_deref(), Some("Hello wor"));
    }

    #[test]
    fn partial_streaming_extracts_response_text_before_tool_name() {
        let partial = r#"{"tool_args":{"text":"Early"},"tool_n"#;
        let p = extract_json_streaming_partial(partial);
        assert_eq!(p.tool_name, None);
        assert_eq!(p.response_text.as_deref(), Some("Early"));
    }

    #[test]
    fn finalize_sidecar_and_primary() {
        let j = r#"{"thoughts":"","headline":"","sidecar_tools":[{"tool_name":"task_board_patch","tool_args":{"items":"[]"}}],"tool_name":"terminal","tool_args":{"command":"echo ok"}}"#;
        let (env, _) = finalize_json_tool_envelope(j, "");
        let env = env.unwrap();
        assert_eq!(env.sidecar.len(), 1);
        assert_eq!(env.sidecar[0].name, "task_board_patch");
        assert_eq!(env.primary.name, "terminal");
    }
}
