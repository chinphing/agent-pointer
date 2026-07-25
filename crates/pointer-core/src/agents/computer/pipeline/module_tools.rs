//! Virtual pipeline tools for Verify LLM phase.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::models::ToolCall;

use super::schemas::verify_response_schema;
use super::types::VerifyModuleOutput;

pub const TOOL_SUBMIT_VERIFY: &str = "submit_verify";

/// OpenAI tool def for a Verify phase call.
pub fn verify_submit_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": TOOL_SUBMIT_VERIFY,
            "description": "Submit verify output after reasoning in reasoning_content per Scenario.",
            "parameters": verify_response_schema()
        }
    })
}

pub fn verify_tools() -> Vec<Value> {
    vec![verify_submit_tool()]
}

fn pick_submit_tool_call<'a>(
    tool_calls: &'a [ToolCall],
    allowed: impl Fn(&str) -> bool,
    phase: &str,
) -> Result<&'a ToolCall> {
    let matches: Vec<&ToolCall> = tool_calls
        .iter()
        .filter(|tc| allowed(tc.name.trim()))
        .collect();
    match matches.len() {
        0 => Err(anyhow!(
            "pipeline {phase}: model returned no submit tool call (tool_calls={})",
            tool_calls.len()
        )),
        1 => Ok(matches[0]),
        n => {
            log::warn!(
                "pipeline {phase}: model returned {n} submit tool calls; using first ({})",
                matches[0].name
            );
            Ok(matches[0])
        }
    }
}

/// Parse the verify submit tool call into [`VerifyModuleOutput`].
pub fn parse_verify_from_tool_calls(tool_calls: &[ToolCall]) -> Result<VerifyModuleOutput> {
    let tc = pick_submit_tool_call(tool_calls, |name| name == TOOL_SUBMIT_VERIFY, "verify")?;
    let args = tc.arguments.trim();
    if args.is_empty() {
        return Err(anyhow!("pipeline verify: empty arguments on submit_verify"));
    }
    serde_json::from_str(args)
        .map_err(|e| anyhow!("pipeline verify: invalid tool arguments: {e}; raw={args}"))
}

/// Serialize tool arguments for pipeline debug UI output pane.
pub fn tool_call_raw_text(tc: &ToolCall) -> String {
    let args = tc.arguments.trim();
    if args.is_empty() {
        return format!("{{\"tool\":\"{}\"}}", tc.name);
    }
    args.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tc(name: &str, args: &str) -> ToolCall {
        ToolCall {
            id: "tc1".into(),
            name: name.into(),
            arguments: args.into(),
            status: "pending".into(),
            result: None,
            error: None,
            duration_ms: None,
            risk_level: None,
            display_label: None,
            display_summary: None,
        }
    }

    #[test]
    fn parses_verify_tool() {
        let v: VerifyModuleOutput = parse_verify_from_tool_calls(&[tc(
            TOOL_SUBMIT_VERIFY,
            r#"{"action_result":"pass","loading_detected":false,"step_summary":"ok"}"#,
        )])
        .unwrap();
        assert_eq!(v.action_result, "pass");
    }
}
