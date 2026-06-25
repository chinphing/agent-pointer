//! Task-board planner loop (Computer lead, pre-execution).

mod history;
mod llm;
mod run;
mod stream_ui;
mod system;
mod tool_pass;
mod tools;

pub use run::{run_planner_loop, PlannerContext, PlannerRunInput};
pub use stream_ui::{exclude_ui_shell_from_lead_context, PlannerUiTarget, PLANNER_PHASE_THOUGHTS};
pub use run::{PlannerRunOutcome, PlannedMethod};
