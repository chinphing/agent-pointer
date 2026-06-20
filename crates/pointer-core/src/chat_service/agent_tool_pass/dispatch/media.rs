//! Image / video generation and media understanding tool dispatch.

use crate::models::ToolCall;
use crate::provider::OpenAIProvider;
use crate::tools::media_generate::{dispatch_media_generate_async, MediaGenerateDispatchContext};
use crate::tools::media_understand::{dispatch_media_understand_async, MediaUnderstandDispatchContext};
use anyhow::anyhow;
use tokio_util::sync::CancellationToken;

use super::super::super::StreamTx;
use super::super::types::ToolExecResult;

pub(super) async fn dispatch_media_generate(
    stream: &StreamTx,
    provider: &OpenAIProvider,
    conversation_id: &str,
    message_id: &str,
    tc: &ToolCall,
    tool_id: &str,
    args_value: serde_json::Value,
    cancel: &CancellationToken,
    lead_run_id: Option<&str>,
    sub_run_id: Option<&str>,
) -> ToolExecResult {
    let run_id = lead_run_id
        .or(sub_run_id)
        .ok_or_else(|| anyhow!("media generation requires lead or sub scope"))?;
    dispatch_media_generate_async(MediaGenerateDispatchContext {
        settings: &provider.settings,
        conversation_id,
        run_id,
        tool_id,
        args: args_value,
        cancel: cancel.clone(),
        stream: stream.clone(),
        message_id: message_id.to_string(),
        tool_call_id: tc.id.clone(),
    })
    .await
}

pub(super) async fn dispatch_media_understand(
    stream: &StreamTx,
    provider: &OpenAIProvider,
    conversation_id: &str,
    message_id: &str,
    tc: &ToolCall,
    args_value: serde_json::Value,
    cancel: &CancellationToken,
    lead_run_id: Option<&str>,
    sub_run_id: Option<&str>,
) -> ToolExecResult {
    let run_id = lead_run_id
        .or(sub_run_id)
        .ok_or_else(|| anyhow!("media_understand requires lead or sub scope"))?;
    dispatch_media_understand_async(MediaUnderstandDispatchContext {
        settings: &provider.settings,
        conversation_id,
        run_id,
        args: args_value,
        cancel: cancel.clone(),
        stream: stream.clone(),
        message_id: message_id.to_string(),
        tool_call_id: tc.id.clone(),
    })
    .await
}
