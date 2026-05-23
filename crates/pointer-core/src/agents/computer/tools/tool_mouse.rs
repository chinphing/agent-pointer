use crate::agents::computer::actions::ActionExecutor;
use crate::agents::computer::tier::{current_computer_tier, tier_allows_index_tools};
use crate::agents::computer::verify::VerifyHintGenerator;
use crate::agents::computer::vision_state::VisionState;
use super::args_util::{
    clamp_scroll_lines, human_like_from_args, method_uses_overlay_index, require_non_empty_str,
    resolve_index_pixels as resolve_index_from_vision, MOVE_OFFSET_MAX,
};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// Execute mouse-related tool calls (aligned with PyProjects/pointer `mouse.py`).
pub struct MouseTool {
    executor: Arc<Mutex<ActionExecutor>>,
    vision_state: Arc<Mutex<VisionState>>,
    verify: VerifyHintGenerator,
    human_like_default: bool,
}

impl MouseTool {
    pub fn new(
        executor: Arc<Mutex<ActionExecutor>>,
        vision_state: Arc<Mutex<VisionState>>,
        human_like_default: bool,
    ) -> Self {
        Self {
            executor,
            vision_state,
            verify: VerifyHintGenerator::new(),
            human_like_default,
        }
    }

    fn human_like(&self, args: &Value) -> bool {
        human_like_from_args(args, self.human_like_default)
    }

    pub fn execute(&self, method: &str, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        if method_uses_overlay_index(method) {
            self.ensure_index_method_allowed(method)?;
        }
        match method {
            "click_index" => self.click_index(args),
            "double_click_index" => self.double_click_index(args),
            "right_click_index" => self.right_click_index(args),
            "hover_index" => self.hover_index(args),
            "click_at" => self.click_at(args),
            "double_click_at" => self.double_click_at(args),
            "right_click_at" => self.right_click_at(args),
            "hover_at" => self.hover_at(args),
            "click_current" => self.click_current(),
            "double_click_current" => self.double_click_current(),
            "right_click_current" => self.right_click_current(),
            "scroll_at_current" => self.scroll_at_current(args),
            "move_offset" => self.move_offset(args),
            "drag_from_to_at" => self.drag_from_to_at(args),
            "drag_from_to_index" => self.drag_from_to_index(args),
            _ => Err(anyhow!(
                "Unknown mouse method: {}. Use click_index, hover_at, drag_from_to_at, …",
                method
            )),
        }
    }

    fn ensure_index_method_allowed(&self, method: &str) -> Result<()> {
        if !method_uses_overlay_index(method) {
            return Ok(());
        }
        if let Some(tier) = current_computer_tier() {
            if !tier_allows_index_tools(tier) {
                anyhow::bail!(
                    "Index-based mouse methods are disabled at tier `{}`; use *_at with session x/y from Overlay reference bboxes.",
                    tier.label()
                );
            }
        }
        Ok(())
    }

    fn resolve_index_pixels(&self, args: &Value, index: u32) -> Result<(i32, i32)> {
        let vision = self.vision_state.lock().unwrap();
        let out = resolve_index_from_vision(&vision, args, index);
        drop(vision);
        out
    }

    fn click_index(&self, args: &Value) -> Result<String> {
        let index = args["index"]
            .as_u64()
            .ok_or_else(|| anyhow!("Missing or invalid 'index' parameter"))? as u32;
        let (x, y) = self.resolve_index_pixels(args, index)?;
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.click_index(x, y, hl)?;
        Ok(self.verify.click_hint(Some(index), None))
    }

    fn double_click_index(&self, args: &Value) -> Result<String> {
        let index = args["index"]
            .as_u64()
            .ok_or_else(|| anyhow!("Missing or invalid 'index' parameter"))? as u32;
        let (x, y) = self.resolve_index_pixels(args, index)?;
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.double_click_index(x, y, hl)?;
        Ok(self.verify.click_hint(Some(index), None))
    }

    fn right_click_index(&self, args: &Value) -> Result<String> {
        let index = args["index"]
            .as_u64()
            .ok_or_else(|| anyhow!("Missing or invalid 'index' parameter"))? as u32;
        let (x, y) = self.resolve_index_pixels(args, index)?;
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.right_click_index(x, y, hl)?;
        Ok(self.verify.click_hint(Some(index), None))
    }

    fn hover_index(&self, args: &Value) -> Result<String> {
        let index = args["index"]
            .as_u64()
            .ok_or_else(|| anyhow!("Missing or invalid 'index' parameter"))? as u32;
        let (x, y) = self.resolve_index_pixels(args, index)?;
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.hover_index(x, y, hl)?;
        Ok(format!("Hovered over element index {}", index))
    }

    fn click_at(&self, args: &Value) -> Result<String> {
        let x = args["x"]
            .as_f64()
            .ok_or_else(|| anyhow!("Missing or invalid 'x' parameter"))? as f32;
        let y = args["y"]
            .as_f64()
            .ok_or_else(|| anyhow!("Missing or invalid 'y' parameter"))? as f32;
        let vision = self.vision_state.lock().unwrap();
        let (px, py) = vision
            .resolve_coordinate(x, y)
            .ok_or_else(|| anyhow!("Screen bounds not set"))?;
        drop(vision);
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.click_at(px, py, hl)?;
        Ok(self.verify.click_hint(None, Some((px, py))))
    }

    fn double_click_at(&self, args: &Value) -> Result<String> {
        let x = args["x"]
            .as_f64()
            .ok_or_else(|| anyhow!("Missing or invalid 'x' parameter"))? as f32;
        let y = args["y"]
            .as_f64()
            .ok_or_else(|| anyhow!("Missing or invalid 'y' parameter"))? as f32;
        let vision = self.vision_state.lock().unwrap();
        let (px, py) = vision
            .resolve_coordinate(x, y)
            .ok_or_else(|| anyhow!("Screen bounds not set"))?;
        drop(vision);
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.double_click_at(px, py, hl)?;
        Ok(self.verify.click_hint(None, Some((px, py))))
    }

    fn right_click_at(&self, args: &Value) -> Result<String> {
        let x = args["x"]
            .as_f64()
            .ok_or_else(|| anyhow!("Missing or invalid 'x' parameter"))? as f32;
        let y = args["y"]
            .as_f64()
            .ok_or_else(|| anyhow!("Missing or invalid 'y' parameter"))? as f32;
        let vision = self.vision_state.lock().unwrap();
        let (px, py) = vision
            .resolve_coordinate(x, y)
            .ok_or_else(|| anyhow!("Screen bounds not set"))?;
        drop(vision);
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.right_click_at(px, py, hl)?;
        Ok(self.verify.click_hint(None, Some((px, py))))
    }

    fn hover_at(&self, args: &Value) -> Result<String> {
        let x = args["x"]
            .as_f64()
            .ok_or_else(|| anyhow!("Missing or invalid 'x' parameter"))? as f32;
        let y = args["y"]
            .as_f64()
            .ok_or_else(|| anyhow!("Missing or invalid 'y' parameter"))? as f32;
        let vision = self.vision_state.lock().unwrap();
        let (px, py) = vision
            .resolve_coordinate(x, y)
            .ok_or_else(|| anyhow!("Screen bounds not set"))?;
        drop(vision);
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.hover_at(px, py, hl)?;
        Ok(format!("Hovered at ({}, {}) px", px, py))
    }

    fn click_current(&self) -> Result<String> {
        let executor = self.executor.lock().unwrap();
        executor.click_here()?;
        Ok(self.verify.click_hint(None, None))
    }

    fn double_click_current(&self) -> Result<String> {
        let executor = self.executor.lock().unwrap();
        executor.double_click_here()?;
        Ok(self.verify.click_hint(None, None))
    }

    fn right_click_current(&self) -> Result<String> {
        let executor = self.executor.lock().unwrap();
        executor.right_click_here()?;
        Ok(self.verify.click_hint(None, None))
    }

    fn scroll_at_current(&self, args: &Value) -> Result<String> {
        let lines_raw = args["lines"]
            .as_i64()
            .ok_or_else(|| anyhow!("Missing or invalid 'lines' parameter"))? as i32;
        let lines = clamp_scroll_lines(lines_raw)?;
        let executor = self.executor.lock().unwrap();
        executor.scroll_at_current(lines)?;
        Ok(self.verify.scroll_hint(lines))
    }

    fn move_offset(&self, args: &Value) -> Result<String> {
        let dx = args["dx"]
            .as_i64()
            .ok_or_else(|| anyhow!("Missing or invalid 'dx' parameter"))? as i32;
        let dy = args["dy"]
            .as_i64()
            .ok_or_else(|| anyhow!("Missing or invalid 'dy' parameter"))? as i32;
        if dx.abs() > MOVE_OFFSET_MAX || dy.abs() > MOVE_OFFSET_MAX {
            return Err(anyhow!(
                "dx/dy must be within [-{0}, {0}]",
                MOVE_OFFSET_MAX
            ));
        }
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.move_offset(dx, dy, hl)?;
        let (nx, ny) = executor.get_position()?;
        Ok(format!(
            "Moved cursor by ({}, {}) px; now at [{}, {}]. Verify result on next screenshot.",
            dx, dy, nx, ny
        ))
    }

    fn drag_from_to_at(&self, args: &Value) -> Result<String> {
        let x1 = args["x1"]
            .as_f64()
            .ok_or_else(|| anyhow!("drag_from_to_at requires x1, y1, x2, y2"))? as f32;
        let y1 = args["y1"]
            .as_f64()
            .ok_or_else(|| anyhow!("drag_from_to_at requires y1"))? as f32;
        let x2 = args["x2"]
            .as_f64()
            .ok_or_else(|| anyhow!("drag_from_to_at requires x2"))? as f32;
        let y2 = args["y2"]
            .as_f64()
            .ok_or_else(|| anyhow!("drag_from_to_at requires y2"))? as f32;
        let vision = self.vision_state.lock().unwrap();
        let (px1, py1) = vision
            .resolve_coordinate(x1, y1)
            .ok_or_else(|| anyhow!("Screen bounds not set"))?;
        let (px2, py2) = vision
            .resolve_coordinate(x2, y2)
            .ok_or_else(|| anyhow!("Screen bounds not set"))?;
        drop(vision);
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.drag_left_from_to(px1, py1, px2, py2, hl)?;
        Ok(format!(
            "Dragged ({}, {}) -> ({}, {}). Verify on next screenshot.",
            px1, py1, px2, py2
        ))
    }

    fn drag_from_to_index(&self, args: &Value) -> Result<String> {
        let from = args["from_index"]
            .as_u64()
            .ok_or_else(|| anyhow!("drag_from_to_index requires from_index and to_index"))?
            as u32;
        let to = args["to_index"]
            .as_u64()
            .ok_or_else(|| anyhow!("drag_from_to_index requires to_index"))? as u32;
        let vision = self.vision_state.lock().unwrap();
        let (x1, y1) = vision
            .resolve_index(from)
            .ok_or_else(|| anyhow!("from_index {} not found", from))?;
        let (x2, y2) = vision
            .resolve_index(to)
            .ok_or_else(|| anyhow!("to_index {} not found", to))?;
        drop(vision);
        if x1 == x2 && y1 == y2 {
            return Err(anyhow!(
                "from_index and to_index resolve to the same pixel; pick different targets."
            ));
        }
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.drag_left_from_to(x1, y1, x2, y2, hl)?;
        Ok(format!(
            "Dragged index {} -> {} ({} ,{}) -> ({}, {}). Verify on next screenshot.",
            from, to, x1, y1, x2, y2
        ))
    }
}
