use super::SkillRegistry;

/// No built-in skills: add skills via external packages / app data, or wire product UI to
/// `enabledSkillIds` when you introduce real skill definitions.
pub fn register_all(_reg: &SkillRegistry) {}
