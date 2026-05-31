use crate::agents::computer::actions::ActionExecutor;
use crate::agents::computer::verify::VerifyHintGenerator;
use crate::agents::computer::vision_state::VisionState;
use super::args_util::{
    clamp_scroll_lines, human_like_from_args, require_non_empty_str, required_f32_arg,
    move_offset_pixels, required_u32_arg, resolve_index_pixels as resolve_index_from_vision,
};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::{Arc, Mutex};

// ── MouseIndexTool ───────────────────────────────────────────────────────────

/// Mouse actions using overlay index numbers.
pub struct MouseIndexTool {
    executor: Arc<Mutex<ActionExecutor>>,
    vision_state: Arc<Mutex<VisionState>>,
    verify: VerifyHintGenerator,
    human_like_default: bool,
}

impl MouseIndexTool {
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

    fn resolve_index(&self, index: u32) -> Result<(i32, i32)> {
        let vision = self.vision_state.lock().unwrap();
        let out = resolve_index_from_vision(&vision, index);
        drop(vision);
        out
    }

    pub fn execute(&self, method: &str, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        match method {
            "click" => self.click(args),
            "double_click" => self.double_click(args),
            "right_click" => self.right_click(args),
            "hover" => self.hover(args),
            "drag_from_to" => self.drag_from_to(args),
            _ => Err(anyhow!(
                "Unknown mouse method: {method}. Use click, double_click, right_click, hover, drag_from_to."
            )),
        }
    }

    fn click(&self, args: &Value) -> Result<String> {
        let index = required_u32_arg(args, "index")?;
        let (x, y) = self.resolve_index(index)?;
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.click_index(x, y, hl)?;
        Ok(self.verify.click_hint(Some(index), None))
    }

    fn double_click(&self, args: &Value) -> Result<String> {
        let index = required_u32_arg(args, "index")?;
        let (x, y) = self.resolve_index(index)?;
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.double_click_index(x, y, hl)?;
        Ok(self.verify.click_hint(Some(index), None))
    }

    fn right_click(&self, args: &Value) -> Result<String> {
        let index = required_u32_arg(args, "index")?;
        let (x, y) = self.resolve_index(index)?;
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.right_click_index(x, y, hl)?;
        Ok(self.verify.click_hint(Some(index), None))
    }

    fn hover(&self, args: &Value) -> Result<String> {
        let index = required_u32_arg(args, "index")?;
        let (x, y) = self.resolve_index(index)?;
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.hover_index(x, y, hl)?;
        Ok(format!("Hovered over element index {}", index))
    }

    fn drag_from_to(&self, args: &Value) -> Result<String> {
        let from = required_u32_arg(args, "from_index")
            .map_err(|_| anyhow!("drag_from_to requires from_index and to_index"))?;
        let to = required_u32_arg(args, "to_index")
            .map_err(|_| anyhow!("drag_from_to requires to_index"))?;
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

// ── MouseAtTool ──────────────────────────────────────────────────────────────

/// Mouse actions using session-normalized coordinates (0-1000).
pub struct MouseAtTool {
    executor: Arc<Mutex<ActionExecutor>>,
    vision_state: Arc<Mutex<VisionState>>,
    verify: VerifyHintGenerator,
    human_like_default: bool,
}

impl MouseAtTool {
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
        match method {
            "click" => self.click(args),
            "double_click" => self.double_click(args),
            "right_click" => self.right_click(args),
            "hover" => self.hover(args),
            "drag_from_to" => self.drag_from_to(args),
            _ => Err(anyhow!(
                "Unknown mouse_at method: {method}. Use click, double_click, right_click, hover, drag_from_to."
            )),
        }
    }

    fn resolve_coord(&self, x: f32, y: f32) -> Result<(i32, i32)> {
        let vision = self.vision_state.lock().unwrap();
        let out = vision
            .resolve_coordinate(x, y)
            .ok_or_else(|| anyhow!("Screen bounds not set"))?;
        drop(vision);
        Ok(out)
    }

    fn click(&self, args: &Value) -> Result<String> {
        let x = required_f32_arg(args, "x")?;
        let y = required_f32_arg(args, "y")?;
        let (px, py) = self.resolve_coord(x, y)?;
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.click_at(px, py, hl)?;
        Ok(self.verify.click_hint(None, Some((px, py))))
    }

    fn double_click(&self, args: &Value) -> Result<String> {
        let x = required_f32_arg(args, "x")?;
        let y = required_f32_arg(args, "y")?;
        let (px, py) = self.resolve_coord(x, y)?;
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.double_click_at(px, py, hl)?;
        Ok(self.verify.click_hint(None, Some((px, py))))
    }

    fn right_click(&self, args: &Value) -> Result<String> {
        let x = required_f32_arg(args, "x")?;
        let y = required_f32_arg(args, "y")?;
        let (px, py) = self.resolve_coord(x, y)?;
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.right_click_at(px, py, hl)?;
        Ok(self.verify.click_hint(None, Some((px, py))))
    }

    fn hover(&self, args: &Value) -> Result<String> {
        let x = required_f32_arg(args, "x")?;
        let y = required_f32_arg(args, "y")?;
        let (px, py) = self.resolve_coord(x, y)?;
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.hover_at(px, py, hl)?;
        Ok(format!("Hovered at ({}, {}) px", px, py))
    }

    fn drag_from_to(&self, args: &Value) -> Result<String> {
        let x1 = required_f32_arg(args, "x1")
            .map_err(|_| anyhow!("drag_from_to requires x1, y1, x2, y2"))?;
        let y1 = required_f32_arg(args, "y1")
            .map_err(|_| anyhow!("drag_from_to requires y1"))?;
        let x2 = required_f32_arg(args, "x2")
            .map_err(|_| anyhow!("drag_from_to requires x2"))?;
        let y2 = required_f32_arg(args, "y2")
            .map_err(|_| anyhow!("drag_from_to requires y2"))?;
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
}

// ── MouseCurrentTool ─────────────────────────────────────────────────────────

/// Mouse actions at current cursor position — no targeting needed.
pub struct MouseCurrentTool {
    executor: Arc<Mutex<ActionExecutor>>,
    verify: VerifyHintGenerator,
}

impl MouseCurrentTool {
    pub fn new(executor: Arc<Mutex<ActionExecutor>>) -> Self {
        Self {
            executor,
            verify: VerifyHintGenerator::new(),
        }
    }

    pub fn execute(&self, method: &str, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        match method {
            "click" => self.click(),
            "double_click" => self.double_click(),
            "right_click" => self.right_click(),
            "scroll" => self.scroll(args),
            "move_offset" => self.move_offset(args),
            _ => Err(anyhow!(
                "Unknown mouse_current method: {method}. Use click, double_click, right_click, scroll, move_offset."
            )),
        }
    }

    fn click(&self) -> Result<String> {
        let executor = self.executor.lock().unwrap();
        executor.click_here()?;
        Ok(self.verify.click_hint(None, None))
    }

    fn double_click(&self) -> Result<String> {
        let executor = self.executor.lock().unwrap();
        executor.double_click_here()?;
        Ok(self.verify.click_hint(None, None))
    }

    fn right_click(&self) -> Result<String> {
        let executor = self.executor.lock().unwrap();
        executor.right_click_here()?;
        Ok(self.verify.click_hint(None, None))
    }

    fn scroll(&self, args: &Value) -> Result<String> {
        let lines_raw = args["lines"]
            .as_i64()
            .ok_or_else(|| anyhow!("Missing or invalid 'lines' parameter"))? as i32;
        let lines = clamp_scroll_lines(lines_raw)?;
        let executor = self.executor.lock().unwrap();
        executor.scroll_at_current(lines)?;
        Ok(self.verify.scroll_hint(lines))
    }

    fn move_offset(&self, args: &Value) -> Result<String> {
        let (ox, oy) = move_offset_pixels(args)?;
        let executor = self.executor.lock().unwrap();
        executor.move_offset(ox, oy, false)?;
        let (nx, ny) = executor.get_position()?;
        Ok(format!(
            "Moved cursor by ({}, {}) px; now at [{}, {}]. Verify result on next screenshot.",
            ox, oy, nx, ny
        ))
    }
}

// ── MouseTool (unified registry entry) ───────────────────────────────────────

/// Unified `mouse` tool: routes `click_at` / `click_index` / `click_current` etc. by method name.
pub struct MouseTool {
    index: MouseIndexTool,
    at: MouseAtTool,
    current: MouseCurrentTool,
}

impl MouseTool {
    pub fn new(
        executor: Arc<Mutex<ActionExecutor>>,
        vision_state: Arc<Mutex<VisionState>>,
        human_like_default: bool,
    ) -> Self {
        Self {
            index: MouseIndexTool::new(
                executor.clone(),
                vision_state.clone(),
                human_like_default,
            ),
            at: MouseAtTool::new(
                executor.clone(),
                vision_state,
                human_like_default,
            ),
            current: MouseCurrentTool::new(executor),
        }
    }

    pub fn execute(&self, args: &Value) -> Result<String> {
        use super::method_route::{route_mouse, MouseBackend};
        let routed = route_mouse(args)?;
        log::info!(
            "mouse: method={} backend={:?}",
            routed.method,
            routed.backend
        );
        match routed.backend {
            MouseBackend::Index => self.index.execute(&routed.method, args),
            MouseBackend::At => self.at.execute(&routed.method, args),
            MouseBackend::Current => self.current.execute(&routed.method, args),
        }
    }
}
