use crate::agents::computer::actions::ActionExecutor;
use crate::agents::computer::verify::VerifyHintGenerator;
use crate::agents::computer::vision_state::VisionState;
use super::args_util::{
    clamp_scroll_lines, ensure_index_method_allowed, human_like_from_args, json_bool_loose,
    require_non_empty_str, required_f32_arg, required_u32_arg, resolve_index_pixels,
    text_from_args,
};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// Composite actions aligned with PyProjects/pointer `composite_action.py` (no `modified_click` here).
pub struct CompositeActionTool {
    executor: Arc<Mutex<ActionExecutor>>,
    vision_state: Arc<Mutex<VisionState>>,
    verify: VerifyHintGenerator,
    human_like_default: bool,
}

impl CompositeActionTool {
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
        ensure_index_method_allowed(method)?;
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
        let index = required_u32_arg(args, "index")?;
        let text = text_from_args(args.get("text"))?;
        let clear_first = json_bool_loose(args.get("clear_first"));
        let auto_enter = json_bool_loose(args.get("auto_enter"));
        let vision = self.vision_state.lock().unwrap();
        let (x, y) = resolve_index_pixels(&vision, args, index)?;
        drop(vision);
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.type_text_at_with_options(x, y, &text, clear_first, auto_enter, hl)?;
        let mut hint = self.verify.type_hint(&text);
        if auto_enter {
            hint.push_str(" Enter key event was dispatched because auto_enter=true; do not press Enter again unless the UI clearly needs it.");
        }
        Ok(hint)
    }

    fn type_text_at(&self, args: &Value) -> Result<String> {
        let x = required_f32_arg(args, "x")?;
        let y = required_f32_arg(args, "y")?;
        let text = text_from_args(args.get("text"))?;
        let clear_first = json_bool_loose(args.get("clear_first"));
        let auto_enter = json_bool_loose(args.get("auto_enter"));
        let vision = self.vision_state.lock().unwrap();
        let (px, py) = vision
            .resolve_coordinate(x, y)
            .ok_or_else(|| anyhow!("Screen bounds not set"))?;
        drop(vision);
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.type_text_at_with_options(px, py, &text, clear_first, auto_enter, hl)?;
        Ok(self.verify.type_hint(&text))
    }

    fn type_text_at_focused(&self, args: &Value) -> Result<String> {
        let text = text_from_args(args.get("text"))?;
        let clear_first = json_bool_loose(args.get("clear_first"));
        let auto_enter = json_bool_loose(args.get("auto_enter"));
        let executor = self.executor.lock().unwrap();
        executor.type_text_focused_with_options(&text, clear_first, auto_enter)?;
        Ok(self.verify.type_hint(&text))
    }

    fn scroll_at_index(&self, args: &Value) -> Result<String> {
        let index = required_u32_arg(args, "index")?;
        let lines_raw = args["lines"]
            .as_i64()
            .ok_or_else(|| anyhow!("Missing or invalid 'lines' parameter"))? as i32;
        let lines = clamp_scroll_lines(lines_raw)?;
        let vision = self.vision_state.lock().unwrap();
        let (x, y) = resolve_index_pixels(&vision, args, index)?;
        drop(vision);
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.scroll_at(x, y, lines, hl)?;
        Ok(self.verify.scroll_hint(lines))
    }
}
