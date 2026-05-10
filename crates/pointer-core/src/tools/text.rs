use super::{ToolEntry, ToolHandler, ToolPrompt, ToolRegistry};
use std::sync::Arc;

const TEXT_PROMPT: &str = include_str!("prompts/text.md");

pub fn register_all(reg: &ToolRegistry) {
    register_text_stats(reg);
}

fn register_text_stats(reg: &ToolRegistry) {
    let h: ToolHandler = Arc::new(|args| {
        let t = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let chars = t.chars().count();
        let words = t.split_whitespace().count();
        let lines = t.lines().count();
        Ok(
            serde_json::json!({"chars":chars,"words":words,"lines":lines,"bytes":t.len()})
                .to_string(),
        )
    });
    reg.register(ToolEntry::new(
        "text_stats",
        "low",
        false,
        serde_json::json!({
            "type":"object",
            "properties":{ "text":{"type":"string"} },
            "required":["text"]
        }),
        "统计文本的字符数、词数、行数与字节数。",
        Some(ToolPrompt {
            system_prompt: TEXT_PROMPT.into(),
        }),
        h,
    ));
}
