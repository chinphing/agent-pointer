//! Coder agent: composed prompts under `prompts/`; coder-scoped tools live here too.

pub mod read_lints;

const CODER_ROLE: &str = include_str!("prompts/role.md");
const CODER_ROUTINE: &str = include_str!("prompts/flow/routine.md");
const CODER_DELEGATION: &str = include_str!("prompts/delegation.md");
const CODER_TASK_BOARD: &str = include_str!("prompts/task_board.md");
const CODER_SCENARIO_IMPL: &str = include_str!("prompts/scenarios/implementation.md");
const CODER_SCENARIO_DEBUG: &str = include_str!("prompts/scenarios/debugging.md");
const CODER_SCENARIO_REFACTOR: &str = include_str!("prompts/scenarios/refactor.md");
const CODER_SCENARIO_DESIGN: &str = include_str!("prompts/scenarios/design_only.md");
const CODER_SCENARIO_SPEC: &str = include_str!("prompts/scenarios/spec_audit.md");

/// Merged `AGENT.md` body for the coder lead (manifest frontmatter stays in `AGENT.md`).
pub fn composed_system_body() -> String {
    super::join_agent_prompt_sections(&[
        CODER_ROLE,
        CODER_ROUTINE,
        CODER_DELEGATION,
        CODER_TASK_BOARD,
        "## Scenario playbooks",
        CODER_SCENARIO_IMPL,
        CODER_SCENARIO_DEBUG,
        CODER_SCENARIO_REFACTOR,
        CODER_SCENARIO_DESIGN,
        CODER_SCENARIO_SPEC,
    ])
}
