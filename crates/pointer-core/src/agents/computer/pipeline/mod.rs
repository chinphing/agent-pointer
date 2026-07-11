//! Host verify pipeline — post-execute screenshot verification for desktop tools.

pub mod debug_ui;
pub mod json_llm;
pub mod module_tools;
pub mod operation;
pub mod position_strategy;
pub mod schemas;
pub mod types;
pub mod vision_pack;
pub mod wire_format;

mod round;

pub use round::{
    find_root_tool_name, run_verify_phase, verify_outcome_for_tier, verify_outcome_struct,
};
pub use types::{AdvancedPipelineSession, PipelineLlmUsageRecorder, VerifyConclusion};
