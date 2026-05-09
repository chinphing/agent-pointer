use super::{ToolHandler, ToolPrompt, ToolRegistry};
use crate::models::ToolDef;
use anyhow::anyhow;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const GENERAL_PROMPT: &str = include_str!("prompts/general.md");

pub fn register_all(reg: &ToolRegistry) {
    register_random_int(reg);
    register_echo(reg);
}

fn register_random_int(reg: &ToolRegistry) {
    let h: ToolHandler = Arc::new(|args| {
        let min = args.get("min").and_then(|v| v.as_i64()).unwrap_or(0);
        let max = args.get("max").and_then(|v| v.as_i64()).unwrap_or(100);
        if min >= max {
            return Err(anyhow!("min 必须小于 max"));
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(1);
        let span = (max - min) as u64;
        let v = min + (nanos as u64 % span) as i64;
        Ok(serde_json::json!({ "value": v, "min": min, "max": max }).to_string())
    });
    reg.register_with_prompt(
        ToolDef {
            name: "random_int".into(),
            description: "生成 [min, max) 范围内的随机整数".into(),
            parameters_schema: serde_json::json!({
                "type":"object",
                "properties":{ "min":{"type":"integer"}, "max":{"type":"integer"} },
                "required":["min","max"]
            }),
            risk_level: "low".into(),
            requires_approval: false,
        },
        Some(ToolPrompt {
            system_prompt: GENERAL_PROMPT.into(),
        }),
        h,
    );
}

fn register_echo(reg: &ToolRegistry) {
    let h: ToolHandler = Arc::new(|args| Ok(serde_json::json!({ "echo": args }).to_string()));
    reg.register_with_prompt(
        ToolDef {
            name: "echo".into(),
            description: "回显传入的参数对象，便于演示工具调用链路。需要用户授权。".into(),
            parameters_schema: serde_json::json!({"type":"object","properties":{}, "additionalProperties":true}),
            risk_level: "medium".into(),
            requires_approval: true,
        },
        Some(ToolPrompt {
            system_prompt: GENERAL_PROMPT.into(),
        }),
        h,
    );
}
