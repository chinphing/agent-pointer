//! Optional **lead** system-prompt override for headless / benchmark runs.
//!
//! The desktop app and the default `pointer-cli` behaviour are untouched: the
//! override is inert until the process owner sets one of the environment
//! variables below (`pointer-cli --prompt-profile` / `--system-prompt-file`).
//!
//! Two orthogonal knobs:
//!
//! * [`PromptProfile`] — which layer set the lead prompt stack uses.
//!   `default` keeps the shipped behaviour; `minimal` swaps the role prompt for
//!   [`MINIMAL_SYSTEM_PROMPT`] and drops the desktop-only scaffolding layers
//!   (tool system appendix, project context, memory, user rules, locale rule).
//! * [`LeadPromptOverride::system_prompt`] — an optional wholesale replacement
//!   for the role prompt text (read from `POINTER_SYSTEM_PROMPT` /
//!   `POINTER_SYSTEM_PROMPT_FILE`).
//!
//! The layer set is chosen by the profile, so a custom prompt text alone keeps
//! the default layers; `pointer-cli` therefore implies `minimal` when
//! `--system-prompt-file` is passed without an explicit `--prompt-profile`.

use super::AgentPlan;

/// Embedded mini-swe-agent-style prompt used by [`PromptProfile::Minimal`].
pub const MINIMAL_SYSTEM_PROMPT: &str = include_str!("minimal_system_prompt.md");

/// `default` | `minimal`.
pub const ENV_PROMPT_PROFILE: &str = "POINTER_PROMPT_PROFILE";
/// Inline replacement for the lead role prompt.
pub const ENV_SYSTEM_PROMPT: &str = "POINTER_SYSTEM_PROMPT";
/// Path to a file holding the replacement lead role prompt.
pub const ENV_SYSTEM_PROMPT_FILE: &str = "POINTER_SYSTEM_PROMPT_FILE";

/// Layer set used for the lead system prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptProfile {
    /// Shipped behaviour: agent role prompt + every desktop scaffolding layer.
    #[default]
    Default,
    /// Headless / benchmark behaviour: one short role prompt, minimal layers.
    Minimal,
}

impl PromptProfile {
    /// Canonical spelling used by the CLI flag and the env variable.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Minimal => "minimal",
        }
    }

    /// Parse a CLI / env value (case-insensitive). Unknown values return `None`.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "default" | "full" => Some(Self::Default),
            "minimal" | "mini" => Some(Self::Minimal),
            _ => None,
        }
    }

    pub fn is_minimal(self) -> bool {
        matches!(self, Self::Minimal)
    }
}

/// Resolved lead prompt override (profile + optional replacement text).
#[derive(Debug, Clone, Default)]
pub struct LeadPromptOverride {
    pub profile: PromptProfile,
    /// When set, replaces the rendered agent role prompt verbatim.
    pub system_prompt: Option<String>,
}

impl LeadPromptOverride {
    /// Resolve from an env-style lookup (`std::env::var` in production).
    ///
    /// An unreadable or empty `POINTER_SYSTEM_PROMPT_FILE` is logged and
    /// ignored, so a typo can never silently blank out the system prompt.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let profile = lookup(ENV_PROMPT_PROFILE)
            .and_then(|raw| PromptProfile::parse(&raw))
            .unwrap_or_default();
        let mut system_prompt = non_empty(lookup(ENV_SYSTEM_PROMPT));
        if system_prompt.is_none() {
            if let Some(path) = non_empty(lookup(ENV_SYSTEM_PROMPT_FILE)) {
                match std::fs::read_to_string(&path) {
                    Ok(text) => match non_empty(Some(text)) {
                        Some(text) => system_prompt = Some(text),
                        None => log::warn!("{ENV_SYSTEM_PROMPT_FILE} {path} is empty; ignoring"),
                    },
                    Err(err) => {
                        log::warn!("failed to read {ENV_SYSTEM_PROMPT_FILE} {path}: {err}");
                    }
                }
            }
        }
        Self {
            profile,
            system_prompt,
        }
    }

    /// Production entry point: resolve from the process environment.
    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// True when the override changes the prompt at all.
    pub fn is_active(&self) -> bool {
        self.profile.is_minimal() || self.system_prompt.is_some()
    }

    /// Replace the lead plan's role prompt and record the active layer set.
    ///
    /// No-op when [`Self::is_active`] is false, so the default path is byte-for-byte
    /// identical to the pre-override behaviour.
    pub fn apply(&self, plan: &mut AgentPlan) {
        if !self.is_active() {
            return;
        }
        plan.prompt_profile = self.profile;
        let text = self
            .system_prompt
            .clone()
            .unwrap_or_else(|| MINIMAL_SYSTEM_PROMPT.trim().to_string());
        plan.system_prompts = vec![text.clone()];
        plan.active_system_prompt = text;
        // The override owns the whole role prompt: skill prompts would be
        // appended by the caller, so drop them from the replaced plan.
        plan.resolved_skill_prompts.clear();
    }
}

fn non_empty(raw: Option<String>) -> Option<String> {
    raw.map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

/// Whether the lead round assembler must emit only the short role prompt plus
/// the environment slice, skipping every desktop scaffolding layer
/// (COMMUNICATION_PUBLIC, tool appendix, memory, user rules, project context,
/// locale rule). The computer profile keeps its own tier prompt path.
pub fn uses_minimal_layers(profile: PromptProfile, lead_profile: &super::AgentProfile) -> bool {
    profile.is_minimal() && *lead_profile != super::AgentProfile::Computer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{AgentOrchestrator, AgentRegistry};

    fn test_plan() -> AgentPlan {
        let agents = AgentRegistry::new();
        crate::agents::register_builtin_agents(&agents);
        let skills = crate::skills::SkillRegistry::new();
        let tools = crate::tools::ToolRegistry::new();
        let store = std::sync::Arc::new(crate::task_board::TaskBoardStore::new());
        crate::tools::builtin::register_all(&tools, store);
        AgentOrchestrator::build_plan(
            &agents,
            &skills,
            &tools,
            &[],
            &std::collections::HashMap::new(),
            Some("coder"),
        )
    }

    #[test]
    fn profile_parse_accepts_known_values_and_rejects_others() {
        assert_eq!(PromptProfile::parse("default"), Some(PromptProfile::Default));
        assert_eq!(PromptProfile::parse(" DEFAULT "), Some(PromptProfile::Default));
        assert_eq!(PromptProfile::parse("minimal"), Some(PromptProfile::Minimal));
        assert_eq!(PromptProfile::parse("MINI"), Some(PromptProfile::Minimal));
        assert_eq!(PromptProfile::parse(""), Some(PromptProfile::Default));
        assert_eq!(PromptProfile::parse("verbose"), None);
        assert_eq!(PromptProfile::Minimal.as_str(), "minimal");
        assert!(PromptProfile::Minimal.is_minimal());
        assert!(!PromptProfile::Default.is_minimal());
    }

    #[test]
    fn unset_environment_is_inactive_and_leaves_the_plan_untouched() {
        let ov = LeadPromptOverride::from_lookup(|_| None);
        assert!(!ov.is_active());
        assert_eq!(ov.profile, PromptProfile::Default);
        assert!(ov.system_prompt.is_none());

        let mut plan = test_plan();
        let before = plan.system_prompts.clone();
        ov.apply(&mut plan);
        assert_eq!(plan.system_prompts, before);
        assert_eq!(plan.prompt_profile, PromptProfile::Default);
    }

    #[test]
    fn minimal_profile_swaps_in_the_embedded_short_prompt() {
        let ov = LeadPromptOverride::from_lookup(|key| {
            (key == ENV_PROMPT_PROFILE).then(|| "minimal".to_string())
        });
        assert!(ov.is_active());

        let mut plan = test_plan();
        assert!(plan.system_prompts.len() >= 1);
        assert_ne!(plan.system_prompts[0], MINIMAL_SYSTEM_PROMPT.trim());

        ov.apply(&mut plan);
        assert_eq!(plan.prompt_profile, PromptProfile::Minimal);
        assert_eq!(plan.system_prompts, vec![MINIMAL_SYSTEM_PROMPT.trim().to_string()]);
        assert_eq!(plan.active_system_prompt, MINIMAL_SYSTEM_PROMPT.trim());
        assert!(plan.resolved_skill_prompts.is_empty());
        // The minimal prompt is small: guard against accidental bloat.
        assert!(MINIMAL_SYSTEM_PROMPT.len() < 1_600, "minimal prompt grew");
    }

    #[test]
    fn inline_system_prompt_replaces_the_role_prompt_verbatim() {
        let ov = LeadPromptOverride::from_lookup(|key| {
            (key == ENV_SYSTEM_PROMPT).then(|| "  You are a tiny agent.  ".to_string())
        });
        assert!(ov.is_active());
        assert_eq!(ov.profile, PromptProfile::Default);

        let mut plan = test_plan();
        ov.apply(&mut plan);
        assert_eq!(plan.system_prompts, vec!["You are a tiny agent.".to_string()]);
        assert_eq!(plan.active_system_prompt, "You are a tiny agent.");
        assert_eq!(plan.prompt_profile, PromptProfile::Default);
    }

    #[test]
    fn unreadable_prompt_file_is_ignored() {
        let ov = LeadPromptOverride::from_lookup(|key| {
            (key == ENV_SYSTEM_PROMPT_FILE).then(|| "/nonexistent/pointer-prompt.md".to_string())
        });
        assert!(!ov.is_active());
        assert!(ov.system_prompt.is_none());
    }

    #[test]
    fn minimal_layers_apply_to_worker_profiles_only() {
        use crate::agents::AgentProfile;
        assert!(uses_minimal_layers(PromptProfile::Minimal, &AgentProfile::Coder));
        assert!(uses_minimal_layers(PromptProfile::Minimal, &AgentProfile::General));
        // Computer keeps its own tier prompt path in every profile.
        assert!(!uses_minimal_layers(PromptProfile::Minimal, &AgentProfile::Computer));
        assert!(!uses_minimal_layers(PromptProfile::Default, &AgentProfile::Coder));
    }
}
