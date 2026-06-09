use crate::channel_outbound::{send_channel_outbound, ChannelOutboundRequest};
use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::Arc;

const CHANNEL_MESSAGE_MD: &str = include_str!("prompts/channel_message.md");
const CHANNEL_MESSAGE_DOC_SOURCE: &str = "tools/prompts/channel_message.md";

fn read_text(args: &Value) -> Option<String> {
    args.get("text")
        .or_else(|| args.get("message"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn collect_media_paths(args: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    for key in ["media", "path", "filePath", "file_path"] {
        if let Some(p) = args.get(key).and_then(|v| v.as_str()) {
            let t = p.trim();
            if !t.is_empty() {
                paths.push(t.to_string());
            }
        }
    }
    if let Some(arr) = args.get("mediaUrls").and_then(|v| v.as_array()) {
        for item in arr {
            if let Some(p) = item.as_str() {
                let t = p.trim();
                if !t.is_empty() {
                    paths.push(t.to_string());
                }
            }
        }
    }
    paths
}

pub fn register(reg: &ToolRegistry) {
    let doc = CHANNEL_MESSAGE_MD.trim();
    let handler: ToolHandler = Arc::new(|args: Value| -> Result<String> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("send")
            .trim()
            .to_ascii_lowercase();
        if action != "send" {
            return Err(anyhow!("channel_message: unsupported action '{action}'"));
        }

        let conversation_id = args
            .get("_conversation_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if conversation_id.is_empty() {
            return Err(anyhow!("channel_message: missing host conversation binding"));
        }

        let text = read_text(&args);
        let media_paths = collect_media_paths(&args);
        if text.is_none() && media_paths.is_empty() {
            return Err(anyhow!(
                "channel_message: provide text and/or media path(s)"
            ));
        }

        send_channel_outbound(ChannelOutboundRequest {
            conversation_id,
            text,
            media_paths,
        })?;

        Ok("IM message sent.".into())
    });

    reg.register(ToolEntry::new(
        "channel_message",
        CHANNEL_MESSAGE_DOC_SOURCE,
        "low",
        false,
        doc,
        handler,
    ));
}
