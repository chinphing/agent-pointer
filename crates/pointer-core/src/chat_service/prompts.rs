use crate::agents::computer::ComputerState;
use crate::agents::{
    expand_agent_prompt_placeholders, rendered_charts_inject, rendered_html_tables_inject,
    rendered_media_delivery_inject, rendered_mermaid_diagrams_inject,
    rendered_svg_diagrams_inject, AgentProfile, SessionInjectVars,
};

/// `MEDIA_DELIVERY` — general / coder / computer.
fn wants_reply_media_prompts(profile: &AgentProfile) -> bool {
    matches!(
        profile,
        AgentProfile::General | AgentProfile::Coder | AgentProfile::Computer
    )
}

/// Charts, Mermaid, SVG, and HTML tables — general / coder only.
fn wants_diagram_prompts(profile: &AgentProfile) -> bool {
    matches!(profile, AgentProfile::General | AgentProfile::Coder)
}

/// Cacheable lead role prompts, in reading order: the agent's own role prompt
/// (which already carries `COMMUNICATION_PUBLIC` between its `AGENT.md` body and
/// its `COMMUNICATION.md`), then the output-format slices.
///
/// The format slices come last because they are delivery mechanics — how to
/// render a chart, a diagram, a file — not how to behave. They used to sit
/// ahead of the role prompt, so the model met `chartjs` fence syntax before it
/// knew what job it was doing.
///
/// Shared by direct single-agent and sub-agent rounds (computer uses the same
/// tier path in both).
pub(crate) fn push_agent_role_cacheable_prompts(
    cacheable: &mut Vec<String>,
    profile: &AgentProfile,
    _computer_state: Option<&ComputerState>,
    _conversation_id: &str,
    session_vars: &SessionInjectVars,
    non_computer_system_prompts: &[String],
) {
    cacheable.extend(
        non_computer_system_prompts
            .iter()
            .map(|p| expand_agent_prompt_placeholders(p, session_vars)),
    );
    if wants_reply_media_prompts(profile) {
        if let Some(block) = rendered_media_delivery_inject() {
            cacheable.push(block);
        }
    }
    if wants_diagram_prompts(profile) {
        if let Some(block) = rendered_charts_inject() {
            cacheable.push(block);
        }
        if let Some(block) = rendered_mermaid_diagrams_inject() {
            cacheable.push(block);
        }
        if let Some(block) = rendered_svg_diagrams_inject() {
            cacheable.push(block);
        }
        if let Some(block) = rendered_html_tables_inject() {
            cacheable.push(block);
        }
    }
}

/// Appends `[Environment]` + **calendar date only** to cacheable system slices.
pub(crate) fn push_env_to_cacheable(system_cacheable: &mut Vec<String>) {
    system_cacheable.push(format!(
        "[Environment]\n{}",
        crate::env_prompt::build_environment_system_prompt_slice()
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_prompt_precedes_output_format_slices() {
        // Format slices (charts, diagrams, media) are delivery mechanics. They
        // used to be pushed ahead of the role prompt, so the model met fence
        // syntax before it knew what job it was doing.
        let mut cacheable = Vec::new();
        push_agent_role_cacheable_prompts(
            &mut cacheable,
            &AgentProfile::General,
            None,
            "",
            &SessionInjectVars {
                workspace_root: "/tmp/ws",
            },
            &["ROLE_PROMPT_SENTINEL".to_string()],
        );
        let role_at = cacheable
            .iter()
            .position(|s| s.contains("ROLE_PROMPT_SENTINEL"))
            .expect("role prompt pushed");
        assert_eq!(role_at, 0, "role prompt must be the first slice");
        assert!(
            cacheable.len() > 1,
            "format slices should follow the role prompt, not vanish"
        );
    }
}
