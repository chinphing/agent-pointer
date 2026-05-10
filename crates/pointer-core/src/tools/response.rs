use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, Result};
use serde_json::json;
use serde_json::Value;
use std::sync::Arc;

const RESPONSE_DOC: &str = include_str!("prompts/response.md");

/// 与 PyProjects/pointer `prompts/agent.system.tool.response.md` 一致：主参数名为 `text`；兼容历史 `message`。
pub(crate) fn response_text_from_args(args: &Value) -> Option<&str> {
    args.get("text")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("message").and_then(|v| v.as_str()))
}

/// 注册 response 工具（文案自 PyProjects/pointer 同步，见 `prompts/response.md`）。
pub fn register_all(reg: &ToolRegistry) {
    let handler: ToolHandler = Arc::new(|args: Value| -> Result<String> {
        let text = response_text_from_args(&args)
            .ok_or_else(|| anyhow!("缺少必需参数: text"))?;

        Ok(format!("已回复用户: {}", text))
    });

    reg.register(ToolEntry::new(
        "response",
        "low",
        false,
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Full answer or result to the user."
                }
            },
            "required": ["text"]
        }),
        RESPONSE_DOC.trim(),
        None,
        handler,
    ));
}
