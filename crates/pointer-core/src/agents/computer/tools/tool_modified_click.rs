use crate::agents::computer::actions::ActionExecutor;
use crate::agents::computer::vision_state::VisionState;
use super::args_util::{
    human_like_from_args, parse_indices, require_non_empty_str,
    value_to_f32_loose,
};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::{Arc, Mutex};

fn parse_click_position(item: &Value) -> Result<(f32, f32)> {
    if let Some(o) = item.as_object() {
        let x = o
            .get("x")
            .and_then(value_to_f32_loose)
            .ok_or_else(|| anyhow!("position object needs x"))?;
        let y = o
            .get("y")
            .and_then(value_to_f32_loose)
            .ok_or_else(|| anyhow!("position object needs y"))?;
        return Ok((x, y));
    }
    if let Some(pair) = item.as_array() {
        if pair.len() < 2 {
            return Err(anyhow!("position [x,y] needs two numbers"));
        }
        let x = value_to_f32_loose(&pair[0]).ok_or_else(|| anyhow!("bad x"))?;
        let y = value_to_f32_loose(&pair[1]).ok_or_else(|| anyhow!("bad y"))?;
        return Ok((x, y));
    }
    Err(anyhow!("each position must be object or [x,y] array"))
}

// ── ModifiedClickIndexTool ───────────────────────────────────────────────────

/// Multi-select clicks using overlay index numbers.
pub struct ModifiedClickIndexTool {
    executor: Arc<Mutex<ActionExecutor>>,
    vision_state: Arc<Mutex<VisionState>>,
    human_like_default: bool,
}

impl ModifiedClickIndexTool {
    pub fn new(
        executor: Arc<Mutex<ActionExecutor>>,
        vision_state: Arc<Mutex<VisionState>>,
        human_like_default: bool,
    ) -> Self {
        Self {
            executor,
            vision_state,
            human_like_default,
        }
    }

    fn human_like(&self, args: &Value) -> bool {
        human_like_from_args(args, self.human_like_default)
    }

    pub fn execute(&self, method: &str, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        match method {
            "select" => self.select(args),
            "range_select" => self.range_select(args),
            _ => Err(anyhow!(
                "Unknown modified_click_index method: {method}. Use select, range_select."
            )),
        }
    }

    fn select(&self, args: &Value) -> Result<String> {
        let indices = parse_indices(args.get("indices"))?;
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
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.click_add_to_selection_batch(&positions, hl)?;
        Ok(format!(
            "Cmd/Ctrl+click multi-select on {} item(s). Verify on next screenshot.",
            positions.len()
        ))
    }

    fn range_select(&self, args: &Value) -> Result<String> {
        let indices = parse_indices(args.get("indices"))?;
        let vision = self.vision_state.lock().unwrap();
        let mut positions: Vec<(i32, i32)> = Vec::new();
        for idx in indices {
            if let Some(p) = vision.resolve_index(idx) {
                positions.push(p);
            }
        }
        drop(vision);
        if positions.len() != 2 {
            return Err(anyhow!(
                "For range_select, indices must resolve to exactly two positions [first, last]."
            ));
        }
        let first = positions[0];
        let last = positions[1];
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.click_range_shift(first, last, hl)?;
        Ok("Shift+click range selection applied. Verify result on next screenshot.".into())
    }
}

// ── ModifiedClickAtTool ──────────────────────────────────────────────────────

/// Multi-select clicks using session-normalized coordinates.
pub struct ModifiedClickAtTool {
    executor: Arc<Mutex<ActionExecutor>>,
    vision_state: Arc<Mutex<VisionState>>,
    human_like_default: bool,
}

impl ModifiedClickAtTool {
    pub fn new(
        executor: Arc<Mutex<ActionExecutor>>,
        vision_state: Arc<Mutex<VisionState>>,
        human_like_default: bool,
    ) -> Self {
        Self {
            executor,
            vision_state,
            human_like_default,
        }
    }

    fn human_like(&self, args: &Value) -> bool {
        human_like_from_args(args, self.human_like_default)
    }

    pub fn execute(&self, method: &str, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        match method {
            "select" => self.select(args),
            "range_select" => self.range_select(args),
            _ => Err(anyhow!(
                "Unknown modified_click_at method: {method}. Use select, range_select."
            )),
        }
    }

    fn select(&self, args: &Value) -> Result<String> {
        let arr = args
            .get("positions")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("Missing 'positions' array"))?;
        let vision = self.vision_state.lock().unwrap();
        let mut positions: Vec<(i32, i32)> = Vec::with_capacity(arr.len());
        for item in arr {
            let (x, y) = parse_click_position(item)?;
            let (nx, ny) = vision
                .resolve_coordinate(x, y)
                .ok_or_else(|| anyhow!("Screen bounds not set"))?;
            positions.push((nx, ny));
        }
        drop(vision);
        if positions.is_empty() {
            return Err(anyhow!("No positions resolved."));
        }
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.click_add_to_selection_batch(&positions, hl)?;
        Ok(format!(
            "Cmd/Ctrl+click {} position(s). Verify on next screenshot.",
            positions.len()
        ))
    }

    fn range_select(&self, args: &Value) -> Result<String> {
        let arr = args
            .get("positions")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("Missing 'positions' array"))?;
        let vision = self.vision_state.lock().unwrap();
        let mut positions: Vec<(i32, i32)> = Vec::with_capacity(arr.len());
        for item in arr {
            let (x, y) = parse_click_position(item)?;
            let (nx, ny) = vision
                .resolve_coordinate(x, y)
                .ok_or_else(|| anyhow!("Screen bounds not set"))?;
            positions.push((nx, ny));
        }
        drop(vision);
        if positions.len() != 2 {
            return Err(anyhow!(
                "For range_select, positions must be exactly two [first, last]."
            ));
        }
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.click_range_shift(positions[0], positions[1], hl)?;
        Ok("Shift+click range by coordinates. Verify result on next screenshot.".into())
    }
}

// ── ModifiedClickTool (unified registry entry) ─────────────────────────────

pub struct ModifiedClickTool {
    index: ModifiedClickIndexTool,
    at: ModifiedClickAtTool,
}

impl ModifiedClickTool {
    pub fn new(
        executor: Arc<Mutex<ActionExecutor>>,
        vision_state: Arc<Mutex<VisionState>>,
        human_like_default: bool,
    ) -> Self {
        Self {
            index: ModifiedClickIndexTool::new(
                executor.clone(),
                vision_state.clone(),
                human_like_default,
            ),
            at: ModifiedClickAtTool::new(executor, vision_state, human_like_default),
        }
    }

    pub fn execute(&self, args: &Value) -> Result<String> {
        use super::method_route::{route_modified_click, ModifiedClickBackend};
        let routed = route_modified_click(args)?;
        log::info!(
            "modified_click: method={} backend={:?}",
            routed.method,
            routed.backend
        );
        match routed.backend {
            ModifiedClickBackend::Index => self.index.execute(&routed.method, args),
            ModifiedClickBackend::At => self.at.execute(&routed.method, args),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_click_position_accepts_object_numeric_strings() {
        let (x, y) = parse_click_position(&json!({"x": "450", "y": "315"})).unwrap();
        assert_eq!(x, 450.0);
        assert_eq!(y, 315.0);
    }

    #[test]
    fn parse_click_position_accepts_array_numeric_strings() {
        let (x, y) = parse_click_position(&json!(["450", "315"])).unwrap();
        assert_eq!(x, 450.0);
        assert_eq!(y, 315.0);
    }

    #[test]
    fn parse_click_position_rejects_invalid_values() {
        assert!(parse_click_position(&json!({"x": "bad", "y": 315})).is_err());
        assert!(parse_click_position(&json!(["450"])).is_err());
    }
}
