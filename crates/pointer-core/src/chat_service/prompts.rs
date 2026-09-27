use crate::agents::computer::ComputerState;
use crate::agents::{
    computer_agent_body_for_tier, computer_communication_for_tier,
    expand_agent_prompt_placeholders, rendered_charts_inject, rendered_communication_public_inject,
    rendered_html_tables_inject, rendered_media_delivery_inject, rendered_mermaid_diagrams_inject,
    rendered_svg_diagrams_inject, AgentProfile, SessionInjectVars,
};

/// `MEDIA_DELIVERY` / `CHARTS` / `SVG_DIAGRAMS` / `HTML_TABLES` — general / coder / computer.
fn wants_reply_media_prompts(profile: &AgentProfile) -> bool {
    matches!(
        profile,
        AgentProfile::General | AgentProfile::Coder | AgentProfile::Computer
    )
}

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
    if wants_reply_media_prompts(profile) {
        if let Some(block) = rendered_media_delivery_inject() {
            cacheable.push(block);
        }
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
/// Reply-language rule follows `UserSettings.uiLocale`.
pub(crate) fn push_env_to_cacheable(system_cacheable: &mut Vec<String>) {
    let ui_locale = crate::storage::load_user_settings()
        .map(|u| u.ui_locale)
        .unwrap_or_else(|e| {
            log::warn!("[env_prompt] load_user_settings for uiLocale failed: {e}");
            "system".into()
        });
    system_cacheable.push(format!(
        "[Environment]\n{}",
        crate::env_prompt::build_environment_system_prompt_slice_for(&ui_locale)
    ));
}
