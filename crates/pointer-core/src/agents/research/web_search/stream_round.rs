//! Research sub-agent SearchAgent round (AGENT.md + history + SSE).

use crate::agents::research::web_search::messages::build_research_sub_agent_messages;
use crate::models::{ChatMessage, ModelSettings};
use anyhow::Result;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::tools::web_search::{
    execute_responses_web_search, parse_research_web_search_args, WebSearchStreamUi, WebSearchResult,
};

pub async fn execute(
    settings: &ModelSettings,
    agent_id: Option<&str>,
    history: &[ChatMessage],
    exclude_message_id: &str,
    args: &Value,
    cancel: CancellationToken,
    ui: WebSearchStreamUi,
) -> Result<WebSearchResult> {
    let mut req = parse_research_web_search_args(args)?;
    req.messages =
        build_research_sub_agent_messages(history, exclude_message_id, &req.query);
    execute_responses_web_search(settings, agent_id, req, cancel, Some(ui)).await
}
