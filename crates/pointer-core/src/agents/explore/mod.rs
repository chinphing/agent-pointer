//! Explore read-only sub-agent: composed prompts under `prompts/`.

const EXPLORE_ROLE: &str = include_str!("prompts/role.md");
const EXPLORE_ROUTER: &str = include_str!("prompts/flow/router.md");
const EXPLORE_FLOW_STANDARD: &str = include_str!("prompts/flow/standard.md");
const EXPLORE_FLOW_NARROW: &str = include_str!("prompts/flow/fast_narrow.md");
const EXPLORE_FLOW_REACH: &str = include_str!("prompts/flow/fast_reachability.md");
const EXPLORE_DELIVERABLE: &str = include_str!("prompts/deliverable.md");
const EXPLORE_IMPACT_SCAN: &str = include_str!("../_shared/exploration/impact_scan.md");
const EXPLORE_HANDOFF: &str = include_str!("../_shared/exploration/handoff_contract.md");
const EXPLORE_TRACE_WHEN: &str = include_str!("../_shared/exploration/trace_when.md");
const EXPLORE_FILE_DISCIPLINE: &str = include_str!("../_shared/exploration/file_discipline.md");
const EXPLORE_SCENARIO_SINGLE: &str = include_str!("prompts/scenarios/single_module_fix.md");
const EXPLORE_SCENARIO_CROSS: &str = include_str!("prompts/scenarios/cross_module_change.md");
const EXPLORE_SCENARIO_ARCH: &str = include_str!("prompts/scenarios/architecture_explain.md");
const EXPLORE_SCENARIO_REACH: &str = include_str!("prompts/scenarios/reachability_audit.md");
const EXPLORE_SCENARIO_SPEC: &str = include_str!("prompts/scenarios/spec_map.md");
const EXPLORE_SCENARIO_DEBUG: &str = include_str!("prompts/scenarios/production_debug.md");
const EXPLORE_SCENARIO_DESIGN: &str = include_str!("prompts/scenarios/design_only.md");

/// Merged `AGENT.md` body for the explore worker (manifest frontmatter stays in `AGENT.md`).
pub fn composed_system_body() -> String {
    super::join_agent_prompt_sections(&[
        EXPLORE_ROLE,
        EXPLORE_ROUTER,
        EXPLORE_FLOW_STANDARD,
        EXPLORE_FLOW_NARROW,
        EXPLORE_FLOW_REACH,
        "## Scenario playbooks",
        EXPLORE_SCENARIO_SINGLE,
        EXPLORE_SCENARIO_CROSS,
        EXPLORE_SCENARIO_ARCH,
        EXPLORE_SCENARIO_REACH,
        EXPLORE_SCENARIO_SPEC,
        EXPLORE_SCENARIO_DEBUG,
        EXPLORE_SCENARIO_DESIGN,
        EXPLORE_IMPACT_SCAN,
        EXPLORE_HANDOFF,
        EXPLORE_TRACE_WHEN,
        EXPLORE_FILE_DISCIPLINE,
        EXPLORE_DELIVERABLE,
    ])
}
