//! UI stream events for in-flight web search (parallel to terminal output deltas).

use crate::chat_service::StreamTx;
use crate::models::{StreamEvent, WebSearchSourceEntry};

use super::WebSearchSource;

#[derive(Clone)]
pub struct WebSearchStreamUi {
    pub stream: StreamTx,
    pub message_id: String,
    pub tool_call_id: String,
    pub trace_id: Option<String>,
}

fn to_entry(s: &WebSearchSource) -> WebSearchSourceEntry {
    WebSearchSourceEntry {
        index: s.index,
        title: s.title.clone(),
        url: s.url.clone(),
        site_name: s.site_name.clone(),
    }
}

impl WebSearchStreamUi {
    pub fn emit_output_delta(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        let _ = self.stream.send(StreamEvent::WebSearchOutputDelta {
            message_id: self.message_id.clone(),
            tool_call_id: self.tool_call_id.clone(),
            text: text.to_string(),
            trace_id: self.trace_id.clone(),
        });
    }

    pub fn emit_sources_ready(&self, sources: &[WebSearchSource], search_count: u32) {
        if sources.is_empty() && search_count == 0 {
            return;
        }
        let _ = self.stream.send(StreamEvent::WebSearchSourcesReady {
            message_id: self.message_id.clone(),
            tool_call_id: self.tool_call_id.clone(),
            sources: sources.iter().map(to_entry).collect(),
            search_count,
            trace_id: self.trace_id.clone(),
        });
    }
}
