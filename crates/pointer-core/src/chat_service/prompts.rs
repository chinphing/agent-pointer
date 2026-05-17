use crate::agents::rendered_json_wire_format_tail_inject;

/// Appends `[Environment]` + **calendar date only** and optional JSON wire tail to **cacheable**
/// system slices (before `before_main_llm_call`; only `[TASK_BOARD]` is per-round dynamic).
pub(crate) fn push_env_and_json_wire_tail_to_cacheable(
    system_cacheable: &mut Vec<String>,
    tools_appendix_enabled: bool,
) {
    system_cacheable.push(format!(
        "[Environment]\n{}",
        crate::env_prompt::build_environment_system_prompt_slice()
    ));
    if tools_appendix_enabled {
        if let Some(block) = rendered_json_wire_format_tail_inject() {
            system_cacheable.push(block);
        }
    }
}
