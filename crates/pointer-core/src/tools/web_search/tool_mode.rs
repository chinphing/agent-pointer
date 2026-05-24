//! Generic `web_search` tool mode: query-only messages + SSE stream.

use crate::models::ModelSettings;
use anyhow::Result;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::client::{execute_web_search_stream, WebSearchMessage};
use super::stream_ui::WebSearchStreamUi;
use super::{parse_tool_web_search_args, WebSearchResult};

pub async fn execute(
    settings: &ModelSettings,
    agent_id: Option<&str>,
    args: &Value,
    cancel: CancellationToken,
    ui: WebSearchStreamUi,
) -> Result<WebSearchResult> {
    let mut req = parse_tool_web_search_args(args)?;
    req.messages = vec![WebSearchMessage {
        role: "user".into(),
        content: req.query.clone(),
    }];
    execute_web_search_stream(settings, agent_id, req, cancel, Some(ui)).await
}
