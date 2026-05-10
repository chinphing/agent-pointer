use crate::agents::computer::actions::ActionExecutor;
use crate::agents::computer::verify::VerifyHintGenerator;
use super::args_util::require_non_empty_str;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// Hotkey tool (PyProjects/pointer `hotkey.py`).
pub struct HotkeyTool {
    executor: Arc<Mutex<ActionExecutor>>,
    verify: VerifyHintGenerator,
}

impl HotkeyTool {
    pub fn new(executor: Arc<Mutex<ActionExecutor>>) -> Self {
        Self {
            executor,
            verify: VerifyHintGenerator::new(),
        }
    }

    pub fn execute(&self, _method: &str, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        let keys_val = args
            .get("keys")
            .ok_or_else(|| anyhow!("Missing 'keys' in tool_args"))?;
        let key_strings = parse_keys_arg(keys_val)?;
        let key_refs: Vec<&str> = key_strings.iter().map(|s| s.as_str()).collect();
        let executor = self.executor.lock().unwrap();
        executor.hotkey(&key_refs)?;
        Ok(self.verify.hotkey_hint(&key_refs))
    }
}

fn parse_keys_arg(keys: &Value) -> Result<Vec<String>> {
    match keys {
        Value::Array(a) => a
            .iter()
            .map(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| anyhow!("Each key must be a string"))
            })
            .collect(),
        Value::String(s) => {
            let t = s.trim();
            if t.starts_with('[') {
                let arr: Vec<Value> =
                    serde_json::from_str(t).map_err(|_| anyhow!("invalid JSON array in keys"))?;
                return parse_keys_arg(&Value::Array(arr));
            }
            Ok(t
                .split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect())
        }
        _ => Err(anyhow!("keys must be an array or string")),
    }
}
