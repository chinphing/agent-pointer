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
    /// Added to streamed source indices so UI matches multi-search global numbering.
    pub citation_base_index: u32,
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
        crate::stream_broadcast::publish_stream(
            &self.stream,
            StreamEvent::WebSearchOutputDelta {
                message_id: self.message_id.clone(),
                tool_call_id: self.tool_call_id.clone(),
                text: text.to_string(),
                trace_id: self.trace_id.clone(),
            },
        );
    }

    pub fn emit_sources_ready(&self, sources: &[WebSearchSource], search_count: u32) {
        if sources.is_empty() && search_count == 0 {
            return;
        }
        let display_sources: Vec<WebSearchSource> = if self.citation_base_index == 0 {
            sources.to_vec()
        } else {
            sources
                .iter()
                .map(|s| WebSearchSource {
                    index: s.index.saturating_add(self.citation_base_index),
                    title: s.title.clone(),
                    url: s.url.clone(),
                    site_name: s.site_name.clone(),
                })
                .collect()
        };
        crate::stream_broadcast::publish_stream(
            &self.stream,
            StreamEvent::WebSearchSourcesReady {
                message_id: self.message_id.clone(),
                tool_call_id: self.tool_call_id.clone(),
                sources: display_sources.iter().map(to_entry).collect(),
                search_count,
                trace_id: self.trace_id.clone(),
            },
        );
    }
}
