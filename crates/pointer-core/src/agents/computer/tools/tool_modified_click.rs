use crate::agents::computer::actions::ActionExecutor;
use crate::agents::computer::vision_state::VisionState;
use super::args_util::{json_bool_loose, parse_indices, require_non_empty_str};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// Modifier multi-select clicks (PyProjects/pointer `modified_click.py`).
pub struct ModifiedClickTool {
    executor: Arc<Mutex<ActionExecutor>>,
    vision_state: Arc<Mutex<VisionState>>,
}

impl ModifiedClickTool {
    pub fn new(
        executor: Arc<Mutex<ActionExecutor>>,
        vision_state: Arc<Mutex<VisionState>>,
    ) -> Self {
        Self {
            executor,
            vision_state,
        }
    }

    pub fn execute(&self, method: &str, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        match method {
            "modified_click_index" => self.modified_click_index(args),
            "modified_click_at" => self.modified_click_at(args),
            _ => Err(anyhow!(
                "Use method 'modified_click_index' or 'modified_click_at'."
            )),
        }
    }

    fn modified_click_index(&self, args: &Value) -> Result<String> {
        let indices = parse_indices(args.get("indices"))?;
        let range_select = json_bool_loose(args.get("range_select"));
        let vision = self.vision_state.lock().unwrap();
        let mut positions: Vec<(i32, i32)> = Vec::new();
        for idx in indices {
            if let Some(p) = vision.resolve_index(idx) {
                positions.push(p);
            }
        }
        drop(vision);
        if positions.is_empty() {
            return Err(anyhow!("No valid indices resolved to positions."));
        }
        let executor = self.executor.lock().unwrap();
        if range_select {
            if positions.len() != 2 {
                return Err(anyhow!(
                    "For range_select, indices must resolve to exactly two positions [first, last]."
                ));
            }
            let first = positions[0];
            let last = positions[1];
            executor.click_range_shift(first, last)?;
            return Ok(
                "Shift+click range selection applied. Verify result on next screenshot.".into(),
            );
        }
        executor.click_add_to_selection_batch(&positions)?;
        Ok(format!(
            "Cmd/Ctrl+click multi-select on {} item(s). Verify on next screenshot.",
            positions.len()
        ))
    }

    fn modified_click_at(&self, args: &Value) -> Result<String> {
        let range_select = json_bool_loose(args.get("range_select"));
        let arr = args
            .get("positions")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("Missing 'positions' array"))?;
        let vision = self.vision_state.lock().unwrap();
        let mut positions: Vec<(i32, i32)> = Vec::with_capacity(arr.len());
        for item in arr {
            let (nx, ny) = if let Some(o) = item.as_object() {
                let x = o
                    .get("x")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| anyhow!("position object needs x"))? as f32;
                let y = o
                    .get("y")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| anyhow!("position object needs y"))? as f32;
                vision
                    .resolve_coordinate(x, y)
                    .ok_or_else(|| anyhow!("Screen bounds not set"))?
            } else if let Some(pair) = item.as_array() {
                if pair.len() < 2 {
                    return Err(anyhow!("position [x,y] needs two numbers"));
                }
                let x = pair[0].as_f64().ok_or_else(|| anyhow!("bad x"))? as f32;
                let y = pair[1].as_f64().ok_or_else(|| anyhow!("bad y"))? as f32;
                vision
                    .resolve_coordinate(x, y)
                    .ok_or_else(|| anyhow!("Screen bounds not set"))?
            } else {
                return Err(anyhow!("each position must be object or [x,y] array"));
            };
            positions.push((nx, ny));
        }
        drop(vision);
        if positions.is_empty() {
            return Err(anyhow!("No positions resolved."));
        }
        let executor = self.executor.lock().unwrap();
        if range_select {
            if positions.len() != 2 {
                return Err(anyhow!(
                    "For range_select, positions must be exactly two [first, last]."
                ));
            }
            executor.click_range_shift(positions[0], positions[1])?;
            return Ok(
                "Shift+click range by coordinates. Verify result on next screenshot.".into(),
            );
        }
        executor.click_add_to_selection_batch(&positions)?;
        Ok(format!(
            "Cmd/Ctrl+click {} position(s). Verify on next screenshot.",
            positions.len()
        ))
    }
}
