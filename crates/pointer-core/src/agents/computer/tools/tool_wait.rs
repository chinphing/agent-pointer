use crate::agents::computer::verify::VerifyHintGenerator;
use super::args_util::{parse_wait_seconds, require_non_empty_str};
use anyhow::Result;
use std::thread;
use std::time::Duration;

/// Wait tool (PyProjects/pointer `wait.py`).
pub struct WaitTool {
    verify: VerifyHintGenerator,
}

impl WaitTool {
    pub fn new() -> Self {
        Self {
            verify: VerifyHintGenerator::new(),
        }
    }

    pub fn execute(&self, _method: &str, args: &serde_json::Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        let sec = parse_wait_seconds(args.get("seconds"))?;
        thread::sleep(Duration::from_secs_f64(sec));
        Ok(self.verify.wait_hint_secs(sec))
    }
}

impl Default for WaitTool {
    fn default() -> Self {
        Self::new()
    }
}
