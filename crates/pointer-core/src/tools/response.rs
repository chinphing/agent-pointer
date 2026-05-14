use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::Arc;

const RESPONSE_MD: &str = include_str!("prompts/response.md");

/// Matches PyProjects/pointer `prompts/agent.system.tool.response.md`: primary arg is `text`; also accepts legacy `message`.
pub(crate) fn response_text_from_args(args: &Value) -> Option<&str> {
    args.get("text")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("message").and_then(|v| v.as_str()))
}

/// Register the `response` tool (copy aligned with PyProjects/pointer; see `prompts/response.md`).
pub fn register_all(reg: &ToolRegistry) {
    let doc = RESPONSE_MD.trim();
    let handler: ToolHandler = Arc::new(|args: Value| -> Result<String> {
        let text = response_text_from_args(&args)
            .ok_or_else(|| anyhow!("缺少必需参数: text"))?;

        Ok(format!("已回复用户: {}", text))
    });

    reg.register(ToolEntry::new(
        "response",
        "low",
        false,
        doc,
        handler,
    ));
}
