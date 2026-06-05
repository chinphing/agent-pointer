/// Appends `[Environment]` + **calendar date only** to cacheable system slices.
pub(crate) fn push_env_to_cacheable(system_cacheable: &mut Vec<String>) {
    system_cacheable.push(format!(
        "[Environment]\n{}",
        crate::env_prompt::build_environment_system_prompt_slice()
    ));
}
