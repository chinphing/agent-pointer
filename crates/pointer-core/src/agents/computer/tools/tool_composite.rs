use crate::agents::computer::actions::ActionExecutor;
use crate::agents::computer::verify::VerifyHintGenerator;
use crate::agents::computer::vision_state::VisionState;
use super::args_util::{
    clamp_scroll_lines, human_like_from_args, json_bool_loose, require_non_empty_str,
    required_f32_arg, required_u32_arg, resolve_index_pixels, text_from_args,
};
use super::method_route::CompositeBackend;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::{Arc, Mutex};

// ── CompositeActionIndexTool ─────────────────────────────────────────────────

/// Type text or scroll at an overlay-indexed element.
pub struct CompositeActionIndexTool {
    executor: Arc<Mutex<ActionExecutor>>,
    vision_state: Arc<Mutex<VisionState>>,
    verify: VerifyHintGenerator,
    human_like_default: bool,
}

impl CompositeActionIndexTool {
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
            "type_text" => self.type_text(args),
            "scroll" => self.scroll(args),
            _ => Err(anyhow!(
                "Unknown composite_action_index method: {method}. Use type_text, scroll."
            )),
        }
    }

    fn type_text(&self, args: &Value) -> Result<String> {
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

    fn scroll(&self, args: &Value) -> Result<String> {
        let index = required_u32_arg(args, "index")?;
        let lines_raw = args["lines"]
            .as_i64()
            .ok_or_else(|| anyhow!("Missing or invalid 'lines' parameter"))? as i32;
        let lines = clamp_scroll_lines(lines_raw)?;
        let vision = self.vision_state.lock().unwrap();
        let (x, y) = resolve_index_pixels(&vision, index)?;
        drop(vision);
        let hl = self.human_like(args);
        let executor = self.executor.lock().unwrap();
        executor.scroll_at(x, y, lines, hl)?;
        Ok(self.verify.scroll_hint(lines))
    }
}

// ── CompositeActionAtTool ────────────────────────────────────────────────────

/// Type text at session-normalized coordinates (0-1000).
pub struct CompositeActionAtTool {
    executor: Arc<Mutex<ActionExecutor>>,
    vision_state: Arc<Mutex<VisionState>>,
    verify: VerifyHintGenerator,
    human_like_default: bool,
}

impl CompositeActionAtTool {
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
            "type_text" => self.type_text(args),
            _ => Err(anyhow!(
                "Unknown composite_action_at method: {method}. Use type_text."
            )),
        }
    }

    fn type_text(&self, args: &Value) -> Result<String> {
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
}

// ── CompositeActionFocusedTool ───────────────────────────────────────────────

/// Type text into the currently focused input field.
pub struct CompositeActionFocusedTool {
    executor: Arc<Mutex<ActionExecutor>>,
    verify: VerifyHintGenerator,
}

impl CompositeActionFocusedTool {
    pub fn new(executor: Arc<Mutex<ActionExecutor>>) -> Self {
        Self {
            executor,
            verify: VerifyHintGenerator::new(),
        }
    }

    pub fn execute(&self, method: &str, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        match method {
            "type_text" => self.type_text(args),
            _ => Err(anyhow!(
                "Unknown composite_action_focused method: {method}. Use type_text."
            )),
        }
    }

    fn type_text(&self, args: &Value) -> Result<String> {
        let text = text_from_args(args.get("text"))?;
        let clear_first = json_bool_loose(args.get("clear_first"));
        let auto_enter = json_bool_loose(args.get("auto_enter"));
        let executor = self.executor.lock().unwrap();
        executor.type_text_focused_with_options(&text, clear_first, auto_enter)?;
        Ok(self.verify.type_hint(&text))
    }
}

// ── CompositeActionTool (unified registry entry) ─────────────────────────────

pub struct CompositeActionTool {
    index: CompositeActionIndexTool,
    at: CompositeActionAtTool,
    focused: CompositeActionFocusedTool,
}

impl CompositeActionTool {
    pub fn new(
        executor: Arc<Mutex<ActionExecutor>>,
        vision_state: Arc<Mutex<VisionState>>,
        human_like_default: bool,
    ) -> Self {
        Self {
            index: CompositeActionIndexTool::new(
                executor.clone(),
                vision_state.clone(),
                human_like_default,
            ),
            at: CompositeActionAtTool::new(executor.clone(), vision_state, human_like_default),
            focused: CompositeActionFocusedTool::new(executor),
        }
    }

    #[allow(dead_code)]
    pub fn execute(&self, args: &Value) -> Result<String> {
        use super::method_route::{route_composite, CompositeBackend};
        let routed = route_composite(args)?;
        log::info!(
            "composite_action: method={} backend={:?}",
            routed.method,
            routed.backend
        );
        match routed.backend {
            CompositeBackend::Index => self.index.execute(&routed.method, args),
            CompositeBackend::At => self.at.execute(&routed.method, args),
            CompositeBackend::Focused => self.focused.execute(&routed.method, args),
        }
    }

    /// Direct dispatch for flat tool names — bypasses `method`-based routing.
    pub fn execute_with(
        &self,
        backend: CompositeBackend,
        method: &str,
        args: &Value,
    ) -> Result<String> {
        match backend {
            CompositeBackend::Index => self.index.execute(method, args),
            CompositeBackend::At => self.at.execute(method, args),
            CompositeBackend::Focused => self.focused.execute(method, args),
        }
    }
}
