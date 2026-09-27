//! DashScope Generation search SSE drain (sources first, then answer deltas).

use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use log::warn;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::client::{parse_search_sse_chunk, SearchSseAccumulator, SearchSseChunk};
use super::stream_ui::WebSearchStreamUi;

/// Events yielded while draining search SSE chunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchStreamEvent {
    SourcesReady {
        sources: Vec<super::WebSearchSource>,
        search_count: u32,
    },
    ContentDelta {
        text: String,
    },
}

/// Apply one parsed SSE chunk to the accumulator and emit UI-oriented events.
pub fn drain_search_sse_chunk(
    acc: &mut SearchSseAccumulator,
    chunk: &SearchSseChunk,
) -> Vec<SearchStreamEvent> {
    let mut out = Vec::new();
    let had_sources = !acc.sources.is_empty();
    acc.apply_chunk(chunk);
    if !had_sources && !acc.sources.is_empty() {
        let search_count = if acc.search_count > 0 {
            acc.search_count
        } else {
            1
        };
        out.push(SearchStreamEvent::SourcesReady {
            sources: acc.sources.clone(),
            search_count,
        });
    }
    if !chunk.content_delta.is_empty() {
        out.push(SearchStreamEvent::ContentDelta {
            text: chunk.content_delta.clone(),
        });
    }
    out
}

fn emit_stream_events(ui: &WebSearchStreamUi, events: &[SearchStreamEvent]) {
    for ev in events {
        match ev {
            SearchStreamEvent::SourcesReady {
                sources,
                search_count,
            } => ui.emit_sources_ready(sources, *search_count),
            SearchStreamEvent::ContentDelta { text } => ui.emit_output_delta(text),
        }
    }
}

fn dashscope_sse_error_from_chunk(body: &Value) -> Option<String> {
    let code = body.get("code").and_then(|c| c.as_str()).unwrap_or("");
    if code.is_empty() || code.eq_ignore_ascii_case("success") {
        return None;
    }
    let message = body
        .get("message")
        .and_then(|m| m.as_str())
        .unwrap_or("DashScope search error");
    Some(format!("{code}: {message}"))
}

/// Read DashScope SSE payload from a complete string (mock / mislabeled content-type).
pub fn read_dashscope_search_sse_from_str(
    body: &str,
    ui: Option<&WebSearchStreamUi>,
) -> Result<SearchSseAccumulator> {
    let mut acc = SearchSseAccumulator::default();
    for line in body.lines() {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }
        let data = match line.strip_prefix("data:") {
            Some(rest) => rest.trim(),
            None => continue,
        };
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let parsed: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(e) => {
                warn!("web_search SSE chunk JSON parse failed: {e}");
                continue;
            }
        };
        if let Some(err) = dashscope_sse_error_from_chunk(&parsed) {
            return Err(anyhow!("DashScope web search failed: {err}"));
        }
        let chunk = parse_search_sse_chunk(&parsed);
        let events = drain_search_sse_chunk(&mut acc, &chunk);
        if let Some(ui_ctx) = ui {
            emit_stream_events(ui_ctx, &events);
        }
    }
    Ok(acc)
}

/// Read DashScope `X-DashScope-SSE` body until stream ends; emit UI events when provided.
pub async fn read_dashscope_search_sse<S, E>(
    mut byte_stream: S,
    cancel: &CancellationToken,
    ui: Option<&WebSearchStreamUi>,
) -> Result<SearchSseAccumulator>
where
    S: futures_util::Stream<Item = Result<bytes::Bytes, E>> + Unpin,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut acc = SearchSseAccumulator::default();
    let mut buf = String::new();

    loop {
        let item = tokio::select! {
            _ = cancel.cancelled() => return Err(anyhow!(crate::i18n::generation_stopped_msg())),
            v = byte_stream.next() => v,
        };
        let chunk = match item {
            Some(Ok(c)) => c,
            Some(Err(e)) => return Err(anyhow!("web search SSE read failed: {e}")),
            None => break,
        };
        buf.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim_end_matches('\r').trim().to_string();
            buf.drain(..=pos);
            if line.is_empty() {
                continue;
            }
            let data = match line.strip_prefix("data:") {
                Some(rest) => rest.trim(),
                None => continue,
            };
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let parsed: Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(e) => {
                    warn!("web_search SSE chunk JSON parse failed: {e}");
                    continue;
                }
            };
            if let Some(err) = dashscope_sse_error_from_chunk(&parsed) {
                return Err(anyhow!("DashScope web search failed: {err}"));
            }
            let chunk = parse_search_sse_chunk(&parsed);
            let events = drain_search_sse_chunk(&mut acc, &chunk);
            if let Some(ui_ctx) = ui {
                emit_stream_events(ui_ctx, &events);
            }
        }
    }

    Ok(acc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn drain_emits_sources_then_deltas() {
        let mut acc = SearchSseAccumulator::default();
        let chunk1 = SearchSseChunk::from_json(&json!({
            "output": {
                "choices": [{ "message": { "content": "" }, "finish_reason": "null" }],
                "search_info": {
                    "search_results": [{
                        "index": 1,
                        "title": "T",
                        "url": "https://example.com"
                    }]
                }
            }
        }));
        let ev1 = drain_search_sse_chunk(&mut acc, &chunk1);
        assert_eq!(ev1.len(), 1);
        assert!(matches!(ev1[0], SearchStreamEvent::SourcesReady { .. }));

        let chunk2 = SearchSseChunk::from_json(&json!({
            "output": {
                "choices": [{ "message": { "content": "Hello" }, "finish_reason": "null" }]
            }
        }));
        let ev2 = drain_search_sse_chunk(&mut acc, &chunk2);
        assert_eq!(ev2.len(), 1);
        assert!(matches!(ev2[0], SearchStreamEvent::ContentDelta { .. }));
        assert_eq!(acc.answer, "Hello");
    }
}
