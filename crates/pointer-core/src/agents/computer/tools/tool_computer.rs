use super::args_util::require_non_empty_str;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

/// Compatibility / documentation tool for `computer:*` calls the model may emit.
pub struct ComputerMetaTool;

impl ComputerMetaTool {
    pub fn execute(&self, method: &str, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        require_non_empty_str(args, "action")?;
        match method {
            "screenshot" => Ok(serde_json::to_string(&json!({
                "ok": true,
                "message": "The annotated desktop for this turn is already in the conversation: the latest user message tagged [CUR_SCREEN] includes the image. Use mouse (e.g. click_index) or composite_action; no separate screenshot is required to see the UI."
            }))?),
            _ => Err(anyhow!(
                "Unknown computer method: {method}. Supported: screenshot (informational; screen is auto-injected each turn)."
            )),
        }
    }
}
