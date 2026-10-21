use super::args_util::{
    human_like_from_args, json_bool_loose, require_non_empty_str, required_f32_arg,
    text_from_args, zoom_flag_from_args,
};
use super::method_route::InputBackend;
use crate::agents::computer::actions::ActionExecutor;
use crate::agents::computer::verify::VerifyHintGenerator;
use crate::agents::computer::vision_state::VisionState;
use anyhow::Result;
use serde_json::Value;
use std::sync::{Arc, Mutex};

// ── InputAtTool (`input_at`) ─────────────────────────────────────────────────

/// Type text at logical screen pixel coordinates.
pub struct InputAtTool {
    executor: Arc<Mutex<ActionExecutor>>,
    vision_state: Arc<Mutex<VisionState>>,
    verify: VerifyHintGenerator,
    human_like_default: bool,
}

impl InputAtTool {
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

    pub fn execute(&self, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        let x = required_f32_arg(args, "x")?;
        let y = required_f32_arg(args, "y")?;
        let text = text_from_args(args.get("text"))?;
        let clear_first = json_bool_loose(args.get("clear_first"));
        let auto_enter = json_bool_loose(args.get("auto_enter"));
        let use_clipboard = args
            .get("use_clipboard")
            .map_or(true, |v| json_bool_loose(Some(v)));
        let use_zoom = zoom_flag_from_args(args)?;
        let vision = self.vision_state.lock().unwrap();
        let (sx, sy) = vision
            .model_point_to_session(x, y, use_zoom)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "zoom is set but this turn has no pointer-zoom crop to map onto the screen"
                )
            })?;
        let (px, py) = vision
            .resolve_coordinate(sx, sy)
            .ok_or_else(|| anyhow::anyhow!("Screen bounds not set"))?;
        drop(vision);
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.type_text_at_with_options(
            px,
            py,
            &text,
            clear_first,
            auto_enter,
            hl,
            use_clipboard,
        )?;
        Ok(self.verify.type_hint(&text))
    }
}

// ── InputFocusedTool (`input_focused`) ───────────────────────────────────────

/// Type text into the currently focused input field.
pub struct InputFocusedTool {
    executor: Arc<Mutex<ActionExecutor>>,
    verify: VerifyHintGenerator,
}

impl InputFocusedTool {
    pub fn new(executor: Arc<Mutex<ActionExecutor>>) -> Self {
        Self {
            executor,
            verify: VerifyHintGenerator::new(),
        }
    }

    pub fn execute(&self, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        let text = text_from_args(args.get("text"))?;
        let clear_first = json_bool_loose(args.get("clear_first"));
        let auto_enter = json_bool_loose(args.get("auto_enter"));
        let use_clipboard = args
            .get("use_clipboard")
            .map_or(true, |v| json_bool_loose(Some(v)));
        let executor = self.executor.lock().unwrap();
        executor.type_text_focused_with_options(&text, clear_first, auto_enter, use_clipboard)?;
        Ok(self.verify.type_hint(&text))
    }
}

// ── InputTool ────────────────────────────────────────────────────────────────

pub struct InputTool {
    at: InputAtTool,
    focused: InputFocusedTool,
}

impl InputTool {
    pub fn new(
        executor: Arc<Mutex<ActionExecutor>>,
        vision_state: Arc<Mutex<VisionState>>,
        human_like_default: bool,
    ) -> Self {
        Self {
            at: InputAtTool::new(executor.clone(), vision_state, human_like_default),
            focused: InputFocusedTool::new(executor),
        }
    }

    pub fn execute_with(&self, backend: InputBackend, args: &Value) -> Result<String> {
        match backend {
            InputBackend::At => self.at.execute(args),
            InputBackend::Focused => self.focused.execute(args),
        }
    }
}
