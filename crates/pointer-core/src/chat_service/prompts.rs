use crate::agents::rendered_json_wire_format_tail_inject;

/// Appends `[Environment]` + **calendar date only** (see [`crate::env_prompt::build_environment_system_prompt_slice`])
/// as the **last** `system_prompts` slice (after `before_main_llm_call` hooks such as `[TASK_BOARD]`).
pub(crate) fn push_env_context_last_in_system_prompts(system_prompts: &mut Vec<String>) {
    system_prompts.push(format!(
        "[Environment]\n{}",
        crate::env_prompt::build_environment_system_prompt_slice()
    ));
}

/// After `[Environment]`, re-state the JSON-only wire contract (recency) when tools are enabled.
pub(crate) fn push_json_wire_format_tail(
    system_prompts: &mut Vec<String>,
    tools_appendix_enabled: bool,
) {
    if !tools_appendix_enabled {
        return;
    }
    if let Some(block) = rendered_json_wire_format_tail_inject() {
        system_prompts.push(block);
    }
}
