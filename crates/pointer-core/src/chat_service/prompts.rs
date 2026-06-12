use crate::agents::computer::ComputerState;
use crate::agents::{
    computer_agent_body_for_tier, computer_communication_for_tier, expand_agent_prompt_placeholders,
    rendered_communication_public_inject, AgentProfile, SessionInjectVars,
};

/// Cacheable lead role prompts: `COMMUNICATION_PUBLIC` + computer tier slice, or non-computer system prompts.
/// Shared by direct single-agent and sub-agent rounds (computer uses the same tier path in both).
pub(crate) fn push_agent_role_cacheable_prompts(
    cacheable: &mut Vec<String>,
    profile: &AgentProfile,
    computer_state: &ComputerState,
    conversation_id: &str,
    session_vars: &SessionInjectVars,
    non_computer_system_prompts: &[String],
) {
    if let Some(block) = rendered_communication_public_inject() {
        cacheable.push(expand_agent_prompt_placeholders(&block, session_vars));
    }
    if *profile == AgentProfile::Computer {
        let tier = computer_state.tier_for_conversation(conversation_id);
        let comm = computer_communication_for_tier(tier);
        let body = computer_agent_body_for_tier(tier);
        let merged = if comm.is_empty() {
            body
        } else if body.is_empty() {
            comm
        } else {
            format!("{comm}\n\n---\n\n{body}")
        };
        if !merged.is_empty() {
            cacheable.push(expand_agent_prompt_placeholders(&merged, session_vars));
        }
    } else {
        cacheable.extend(
            non_computer_system_prompts
                .iter()
                .map(|p| expand_agent_prompt_placeholders(p, session_vars)),
        );
    }
}

/// Appends `[Environment]` + **calendar date only** to cacheable system slices.
pub(crate) fn push_env_to_cacheable(system_cacheable: &mut Vec<String>) {
    system_cacheable.push(format!(
        "[Environment]\n{}",
        crate::env_prompt::build_environment_system_prompt_slice()
    ));
}
