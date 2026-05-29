use crate::agents::rendered_json_wire_format_tail_inject;

/// Appends `[Environment]` + **calendar date only** to cacheable system slices.
pub(crate) fn push_env_to_cacheable(
    system_cacheable: &mut Vec<String>,
    legacy_json_wire_tail_enabled: bool,
) {
    system_cacheable.push(format!(
        "[Environment]\n{}",
        crate::env_prompt::build_environment_system_prompt_slice()
    ));
    if legacy_json_wire_tail_enabled {
        if let Some(block) = rendered_json_wire_format_tail_inject() {
            system_cacheable.push(block);
        }
    }
}
