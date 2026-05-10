use crate::agents::computer::actions::ActionExecutor;
use crate::agents::computer::verify::VerifyHintGenerator;
use crate::agents::computer::vision_state::VisionState;
use super::args_util::{clamp_scroll_lines, json_bool_loose, require_non_empty_str};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// Composite actions aligned with PyProjects/pointer `composite_action.py` (no `modified_click` here).
pub struct CompositeActionTool {
    executor: Arc<Mutex<ActionExecutor>>,
    vision_state: Arc<Mutex<VisionState>>,
    verify: VerifyHintGenerator,
}

impl CompositeActionTool {
    pub fn new(
        executor: Arc<Mutex<ActionExecutor>>,
        vision_state: Arc<Mutex<VisionState>>,
    ) -> Self {
        Self {
            executor,
            vision_state,
            verify: VerifyHintGenerator::new(),
        }
    }

    pub fn execute(&self, method: &str, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        match method {
            "type_text_at_index" => self.type_text_at_index(args),
            "type_text_at" => self.type_text_at(args),
            "type_text_at_focused" => self.type_text_at_focused(args),
            "scroll_at_index" => self.scroll_at_index(args),
            _ => Err(anyhow!(
                "Unknown composite_action method: {}. Use type_text_at_index, type_text_at, type_text_at_focused, scroll_at_index.",
                method
            )),
        }
    }

    fn type_text_at_index(&self, args: &Value) -> Result<String> {
        let index = args["index"]
            .as_u64()
            .ok_or_else(|| anyhow!("Missing or invalid 'index' parameter"))? as u32;
        let text = args["text"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing or invalid 'text' parameter"))?;
        let clear_first = json_bool_loose(args.get("clear_first"));
        let auto_enter = json_bool_loose(args.get("auto_enter"));
        let vision = self.vision_state.lock().unwrap();
        let (x, y) = vision
            .resolve_index(index)
            .ok_or_else(|| anyhow!("Index {} not found in current annotation", index))?;
        drop(vision);
        let executor = self.executor.lock().unwrap();
        executor.type_text_at_with_options(x, y, text, clear_first, auto_enter)?;
        let mut hint = self.verify.type_hint(text);
        if auto_enter {
            hint.push_str(" Enter was sent if auto_enter=true; do not press Enter again unless the UI clearly needs it.");
        }
        Ok(hint)
    }

    fn type_text_at(&self, args: &Value) -> Result<String> {
        let x = args["x"]
            .as_f64()
            .ok_or_else(|| anyhow!("Missing or invalid 'x' parameter"))? as f32;
        let y = args["y"]
            .as_f64()
            .ok_or_else(|| anyhow!("Missing or invalid 'y' parameter"))? as f32;
        let text = args["text"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing or invalid 'text' parameter"))?;
        let clear_first = json_bool_loose(args.get("clear_first"));
        let auto_enter = json_bool_loose(args.get("auto_enter"));
        let vision = self.vision_state.lock().unwrap();
        let (px, py) = vision
            .resolve_coordinate(x, y)
            .ok_or_else(|| anyhow!("Screen bounds not set"))?;
        drop(vision);
        let executor = self.executor.lock().unwrap();
        executor.type_text_at_with_options(px, py, text, clear_first, auto_enter)?;
        Ok(self.verify.type_hint(text))
    }

    fn type_text_at_focused(&self, args: &Value) -> Result<String> {
        let text = args["text"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing or invalid 'text' parameter"))?;
        let clear_first = json_bool_loose(args.get("clear_first"));
        let auto_enter = json_bool_loose(args.get("auto_enter"));
        let executor = self.executor.lock().unwrap();
        executor.type_text_focused_with_options(text, clear_first, auto_enter)?;
        Ok(self.verify.type_hint(text))
    }

    fn scroll_at_index(&self, args: &Value) -> Result<String> {
        let index = args["index"]
            .as_u64()
            .ok_or_else(|| anyhow!("Missing or invalid 'index' parameter"))? as u32;
        let lines_raw = args["lines"]
            .as_i64()
            .ok_or_else(|| anyhow!("Missing or invalid 'lines' parameter"))? as i32;
        let lines = clamp_scroll_lines(lines_raw)?;
        let vision = self.vision_state.lock().unwrap();
        let (x, y) = vision
            .resolve_index(index)
            .ok_or_else(|| anyhow!("Index {} not found in current annotation", index))?;
        drop(vision);
        let executor = self.executor.lock().unwrap();
        executor.scroll_at(x, y, lines)?;
        Ok(self.verify.scroll_hint(lines))
    }
}
