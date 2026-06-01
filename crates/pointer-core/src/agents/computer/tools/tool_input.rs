use crate::agents::computer::actions::ActionExecutor;
use crate::agents::computer::verify::VerifyHintGenerator;
use crate::agents::computer::vision_state::VisionState;
use super::args_util::{
    human_like_from_args, json_bool_loose, require_non_empty_str, required_f32_arg,
    required_u32_arg, resolve_index_pixels, text_from_args,
};
use super::method_route::InputBackend;
use anyhow::Result;
use serde_json::Value;
use std::sync::{Arc, Mutex};

// ── InputIndexTool (`input_index`) ───────────────────────────────────────────

/// Type text at an overlay-indexed element.
pub struct InputIndexTool {
    executor: Arc<Mutex<ActionExecutor>>,
    vision_state: Arc<Mutex<VisionState>>,
    verify: VerifyHintGenerator,
    human_like_default: bool,
}

impl InputIndexTool {
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
        let index = required_u32_arg(args, "index")?;
        let text = text_from_args(args.get("text"))?;
        let clear_first = json_bool_loose(args.get("clear_first"));
        let auto_enter = json_bool_loose(args.get("auto_enter"));
        let vision = self.vision_state.lock().unwrap();
        let (x, y) = resolve_index_pixels(&vision, index)?;
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
}

// ── InputAtTool (`input_at`) ─────────────────────────────────────────────────

/// Type text at session-normalized coordinates (0-1000).
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
        let vision = self.vision_state.lock().unwrap();
        let (px, py) = vision
            .resolve_coordinate(x, y)
            .ok_or_else(|| anyhow::anyhow!("Screen bounds not set"))?;
        drop(vision);
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.type_text_at_with_options(px, py, &text, clear_first, auto_enter, hl)?;
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
        let executor = self.executor.lock().unwrap();
        executor.type_text_focused_with_options(&text, clear_first, auto_enter)?;
        Ok(self.verify.type_hint(&text))
    }
}

// ── InputTool ────────────────────────────────────────────────────────────────

pub struct InputTool {
    index: InputIndexTool,
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
            index: InputIndexTool::new(
                executor.clone(),
                vision_state.clone(),
                human_like_default,
            ),
            at: InputAtTool::new(executor.clone(), vision_state, human_like_default),
            focused: InputFocusedTool::new(executor),
        }
    }

    pub fn execute_with(&self, backend: InputBackend, args: &Value) -> Result<String> {
        match backend {
            InputBackend::Index => self.index.execute(args),
            InputBackend::At => self.at.execute(args),
            InputBackend::Focused => self.focused.execute(args),
        }
    }
}
