use crate::skills::SkillRegistry;
use crate::storage;
use crate::tools::ToolRegistry;
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Computer agent: `state` / `input` / `vision` / `tier`, tools, prompts (`AGENT.md` + `prompts/tiers/*`).
pub mod computer;

/// Coder agent: embedded policy (`AGENT.md`); tools used only by the coder lead (e.g. `read_lints`).
pub mod coder;

/// Read-only explore sub-agent for delegated breadth reconnaissance.
pub mod explore;

/// Deep research agent runtime (SearchAgent web_search path, extension hooks).
pub mod research;

pub mod agent_ui;
pub use agent_ui::{agent_display_label, resolve_agent_ui, AgentUiConfig, ResolvedAgentUi};

pub const AGENT_MODE_SINGLE: &str = "single";
pub const AGENT_MODE_SUPERVISOR: &str = "supervisor";
pub const DEFAULT_AGENT_ID: &str = "general";
/// Default worker selected in single-agent mode when `leadAgentId` is unset.
pub const DEFAULT_LEAD_AGENT_ID: &str = "general";
pub const SUPERVISOR_AGENT_ID: &str = "supervisor";
const AGENTS_DIR: &str = "agents";
const AGENT_MANIFEST: &str = "AGENT.md";
const AGENT_COMMUNICATION: &str = "COMMUNICATION.md";
/// Legacy per-request block; if present, merged into `COMMUNICATION.md` content at load (placeholders expanded each request).
const AGENT_SESSION_INJECT: &str = "SESSION_INJECT.md";

/// Model-facing shared rules: host context, skills, **`thoughts`** meaning, and final-reply discipline (English).
const COMMUNICATION_PUBLIC: &str = include_str!("_shared/COMMUNICATION_PUBLIC.md");
/// Authoritative rules for assistant `MEDIA:` delivery (App, IM, final-reply tools, terminal output).
const MEDIA_DELIVERY: &str = include_str!("_shared/MEDIA_DELIVERY.md");
const COMPUTER_COMMUNICATION_PRIMARY: &str =
    include_str!("computer/prompts/tiers/primary/communication.md");
const COMPUTER_UI_DISABLED_CONTROLS: &str =
    include_str!("computer/prompts/ui_disabled_controls.md");
const COMPUTER_AGENT_PRIMARY: &str = include_str!("computer/prompts/tiers/primary/loop.md");
const COMPUTER_OS_PROMPT_MACOS: &str = include_str!("computer/prompts/os/macos.md");
const COMPUTER_OS_PROMPT_WINDOWS: &str = include_str!("computer/prompts/os/windows.md");
const COMPUTER_OS_PROMPT_LINUX: &str = include_str!("computer/prompts/os/linux.md");
const COMPUTER_VERIFY_CORE: &str = include_str!("computer/prompts/modules/verify/core.md");
const COMPUTER_VERIFY_POINTER_CLICK: &str =
    include_str!("computer/prompts/modules/verify/pointer_click.md");
const COMPUTER_VERIFY_POINTER_HOVER: &str =
    include_str!("computer/prompts/modules/verify/pointer_hover.md");
const COMPUTER_VERIFY_SCROLL: &str = include_str!("computer/prompts/modules/verify/scroll.md");
const COMPUTER_VERIFY_DRAG: &str = include_str!("computer/prompts/modules/verify/drag.md");
const COMPUTER_VERIFY_INPUT: &str = include_str!("computer/prompts/modules/verify/input.md");
const COMPUTER_VERIFY_MODIFIED_CLICK: &str =
    include_str!("computer/prompts/modules/verify/modified_click.md");
const COMPUTER_VERIFY_CAPTCHA: &str = include_str!("computer/prompts/modules/verify/captcha.md");
const COMPUTER_VERIFY_HOTKEY: &str = include_str!("computer/prompts/modules/verify/hotkey.md");
const COMPUTER_VERIFY_WAIT: &str = include_str!("computer/prompts/modules/verify/wait.md");
const COMPUTER_VERIFY_CLIPBOARD: &str =
    include_str!("computer/prompts/modules/verify/clipboard.md");
const COMPUTER_VERIFY_APP_ACCESS: &str =
    include_str!("computer/prompts/modules/verify/app_access.md");
const COMPUTER_VERIFY_GENERIC: &str = include_str!("computer/prompts/modules/verify/generic.md");

/// Injected on **every** main-LLM and sub-agent round (see `chat_service`).
pub fn communication_public_md() -> &'static str {
    COMMUNICATION_PUBLIC.trim()
}

/// Authoritative assistant media delivery rules (injected on every main/sub-agent round).
pub fn media_delivery_md() -> &'static str {
    MEDIA_DELIVERY.trim()
}

pub fn rendered_media_delivery_inject() -> Option<String> {
    let md = media_delivery_md();
    (!md.is_empty()).then(|| md.to_string())
}

/// Join model-facing prompt slices with `\n\n---\n\n` between non-empty sections.
pub(crate) fn join_agent_prompt_sections(sections: &[&str]) -> String {
    let mut parts = Vec::new();
    for section in sections {
        let trimmed = section.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed);
        }
    }
    parts.join("\n\n---\n\n")
}

struct BuiltinAgentBundle {
    id: &'static str,
    manifest: &'static str,
    communication: &'static str,
}

const BUILTIN_AGENT_BUNDLES: &[BuiltinAgentBundle] = &[
    BuiltinAgentBundle {
        id: "general",
        manifest: include_str!("general/AGENT.md"),
        communication: include_str!("general/COMMUNICATION.md"),
    },
    BuiltinAgentBundle {
        id: "supervisor",
        manifest: include_str!("supervisor/AGENT.md"),
        communication: include_str!("supervisor/COMMUNICATION.md"),
    },
    BuiltinAgentBundle {
        id: "coder",
        manifest: include_str!("coder/AGENT.md"),
        communication: include_str!("coder/COMMUNICATION.md"),
    },
    BuiltinAgentBundle {
        id: "explore",
        manifest: include_str!("explore/AGENT.md"),
        communication: include_str!("explore/COMMUNICATION.md"),
    },
    BuiltinAgentBundle {
        id: "research",
        manifest: include_str!("research/AGENT.md"),
        communication: include_str!("research/COMMUNICATION.md"),
    },
    BuiltinAgentBundle {
        id: "computer",
        manifest: include_str!("computer/AGENT.md"),
        communication: "",
    },
];

fn computer_os_prompt_md_for_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        COMPUTER_OS_PROMPT_MACOS
    } else if cfg!(target_os = "windows") {
        COMPUTER_OS_PROMPT_WINDOWS
    } else {
        COMPUTER_OS_PROMPT_LINUX
    }
}

fn builtin_computer_communication() -> String {
    computer_communication_for_tier(computer::tier::ComputerTier::Primary)
}

/// Merged communication + OS slice for a computer tier (cacheable system prefix).
/// All tiers share Primary prompts; only per-tier LLM model differs at runtime.
pub fn computer_communication_for_tier(tier: computer::tier::ComputerTier) -> String {
    let _ = tier;
    let mut parts: Vec<&str> = Vec::new();
    push_trimmed(&mut parts, COMPUTER_COMMUNICATION_PRIMARY);
    push_trimmed(&mut parts, COMPUTER_UI_DISABLED_CONTROLS);
    push_trimmed(&mut parts, computer_os_prompt_md_for_platform());
    parts.join("\n\n---\n\n")
}

/// Agent loop body for a computer tier (merged into system prompt with communication).
pub fn computer_agent_body_for_tier(tier: computer::tier::ComputerTier) -> String {
    let _ = tier;
    COMPUTER_AGENT_PRIMARY.trim().to_string()
}

/// Verify module prompt for an operation family (host post-execute pipeline).
pub fn computer_verify_prompt(family: computer::pipeline::operation::OperationFamily) -> String {
    let family_md = match family {
        computer::pipeline::operation::OperationFamily::PointerClick => {
            COMPUTER_VERIFY_POINTER_CLICK
        }
        computer::pipeline::operation::OperationFamily::PointerHover => {
            COMPUTER_VERIFY_POINTER_HOVER
        }
        computer::pipeline::operation::OperationFamily::Scroll => COMPUTER_VERIFY_SCROLL,
        computer::pipeline::operation::OperationFamily::Drag => COMPUTER_VERIFY_DRAG,
        computer::pipeline::operation::OperationFamily::Input => COMPUTER_VERIFY_INPUT,
        computer::pipeline::operation::OperationFamily::ModifiedClick => {
            COMPUTER_VERIFY_MODIFIED_CLICK
        }
        computer::pipeline::operation::OperationFamily::Captcha => COMPUTER_VERIFY_CAPTCHA,
        computer::pipeline::operation::OperationFamily::Hotkey => COMPUTER_VERIFY_HOTKEY,
        computer::pipeline::operation::OperationFamily::Wait => COMPUTER_VERIFY_WAIT,
        computer::pipeline::operation::OperationFamily::Clipboard => COMPUTER_VERIFY_CLIPBOARD,
        computer::pipeline::operation::OperationFamily::AppAccess => COMPUTER_VERIFY_APP_ACCESS,
        _ => COMPUTER_VERIFY_GENERIC,
    };
    join_agent_prompt_sections(&[COMPUTER_VERIFY_CORE, family_md])
}

fn push_trimmed(parts: &mut Vec<&str>, s: &'static str) {
    let t = s.trim();
    if !t.is_empty() {
        parts.push(t);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentProfile {
    General,
    Supervisor,
    Planner,
    Coder,
    Writer,
    Analyst,
    ToolUser,
    Computer,
    Explore,
    Custom(String),
}

thread_local! {
    static FILE_TOOL_LEAD_PROFILE: RefCell<Option<AgentProfile>> = const { RefCell::new(None) };
}

/// Thread-local lead profile for synchronous `file` tool handlers (set around `ToolRegistry::invoke`).
pub struct FileToolLeadProfileGuard {
    previous: Option<AgentProfile>,
}

impl FileToolLeadProfileGuard {
    pub fn enter(profile: AgentProfile) -> Self {
        let previous = FILE_TOOL_LEAD_PROFILE.with(|c| {
            let mut g = c.borrow_mut();
            std::mem::replace(&mut *g, Some(profile))
        });
        Self { previous }
    }
}

impl Drop for FileToolLeadProfileGuard {
    fn drop(&mut self) {
        FILE_TOOL_LEAD_PROFILE.with(|c| {
            *c.borrow_mut() = self.previous.take();
        });
    }
}

/// Current lead agent profile for the `file` tool on this thread, if any.
pub fn current_file_tool_lead_profile() -> Option<AgentProfile> {
    FILE_TOOL_LEAD_PROFILE.with(|c| c.borrow().clone())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccessPolicy {
    #[serde(default, rename = "allowTools")]
    pub allow_tools: Vec<String>,
    #[serde(default, rename = "denyTools")]
    pub deny_tools: Vec<String>,
    /// Legacy field; ignored at runtime. Skill boundary is defaultSkillIds + override.
    #[serde(default, rename = "allowSkills")]
    pub allow_skills: Vec<String>,
    /// Legacy field; ignored at runtime. Skill boundary is defaultSkillIds + override.
    #[serde(default, rename = "denySkills")]
    pub deny_skills: Vec<String>,
}

/// How an agent resolves its skill set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillsPolicy {
    /// Agent does not load any skills.
    #[serde(rename = "disabled")]
    Disabled,
    /// Agent only uses its `default_skill_ids` — user/conv cannot add.
    #[serde(rename = "defaultsOnly")]
    DefaultsOnly,
    /// Agent lets user configure skills (via `agentSkillOverrides`); falls back to defaults.
    #[serde(rename = "userConfigurable")]
    UserConfigurable,
    /// Sub-agent inherits its parent's resolved skill list.
    #[serde(rename = "inheritsFromParent")]
    InheritsFromParent,
}

impl Default for SkillsPolicy {
    fn default() -> Self {
        Self::Disabled
    }
}

fn is_default_skills_policy(p: &SkillsPolicy) -> bool {
    *p == SkillsPolicy::Disabled
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDef {
    pub id: String,
    pub name: String,
    pub description: String,
    pub role: String,
    pub profile: AgentProfile,
    #[serde(default, rename = "defaultSkillIds")]
    pub default_skill_ids: Vec<String>,
    /// Rules for how this agent resolves its skills.
    #[serde(default, skip_serializing_if = "is_default_skills_policy")]
    pub skills_policy: SkillsPolicy,
    #[serde(default, rename = "accessPolicy")]
    pub access_policy: AccessPolicy,
    #[serde(default)]
    pub builtin: bool,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, rename = "toolNames")]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default, rename = "resourceFiles")]
    pub resource_files: Vec<String>,
    /// Worker ids this lead may pass to `run_subagent`; metadata for these ids is injected at runtime.
    #[serde(default, rename = "allowAgents")]
    pub allow_agents: Vec<String>,
    /// Agent-specific configuration key-value pairs.
    #[serde(default)]
    pub config: HashMap<String, String>,
    /// Optional chat UI visibility overrides.
    #[serde(default)]
    pub ui: AgentUiConfig,
}

#[derive(Debug, Clone)]
pub struct AgentPlan {
    pub mode: String,
    pub lead_agent_id: String,
    pub lead_agent_name: String,
    pub system_prompts: Vec<String>,
    pub active_def: AgentDef,
    pub active_system_prompt: String,
    pub resolved_skill_ids: Vec<String>,
    pub resolved_skill_prompts: Vec<String>,
    pub allowed_tool_names: Vec<String>,
    /// Sorted, deduped worker ids from the lead agent manifest `allowAgents`.
    pub allow_agents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTask {
    pub id: String,
    #[serde(rename = "agentId")]
    pub agent_id: String,
    #[serde(default)]
    pub title: String,
    pub goal: String,
    #[serde(default)]
    pub context: String,
    #[serde(default, rename = "dependsOn")]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunResult {
    #[serde(rename = "taskId")]
    pub task_id: String,
    #[serde(rename = "agentId")]
    pub agent_id: String,
    #[serde(rename = "agentName")]
    pub agent_name: String,
    pub content: String,
    #[serde(default)]
    pub reasoning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentRunLimits {
    pub max_sub_agents: usize,
    pub max_rounds: usize,
    pub timeout_ms: u64,
}

impl Default for AgentRunLimits {
    fn default() -> Self {
        Self {
            max_sub_agents: 4,
            max_rounds: 1,
            timeout_ms: 120_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BaseAgent {
    pub def: AgentDef,
    pub system_prompt: String,
}

#[derive(Debug, Deserialize)]
struct AgentManifest {
    id: String,
    name: String,
    description: String,
    #[serde(default = "default_worker_role")]
    role: String,
    #[serde(default)]
    profile: Option<AgentProfile>,
    #[serde(default, rename = "defaultSkillIds")]
    default_skill_ids: Vec<String>,
    #[serde(default, rename = "skillsPolicy")]
    skills_policy: SkillsPolicy,
    #[serde(default, rename = "accessPolicy")]
    access_policy: AccessPolicy,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default, rename = "toolNames")]
    tool_names: Vec<String>,
    #[serde(default, rename = "allowAgents")]
    allow_agents: Vec<String>,
    #[serde(default)]
    config: HashMap<String, String>,
    #[serde(default)]
    ui: AgentUiConfig,
    #[serde(skip)]
    body: String,
}

pub trait AgentExecutor: Send + Sync {
    fn def(&self) -> AgentDef;
    fn system_prompt(&self) -> String;

    fn profile(&self) -> AgentProfile {
        self.def().profile
    }

    fn access_policy(&self) -> AccessPolicy {
        self.def().access_policy
    }
}

impl AgentExecutor for BaseAgent {
    fn def(&self) -> AgentDef {
        self.def.clone()
    }

    fn system_prompt(&self) -> String {
        self.system_prompt.clone()
    }
}

/// Placeholders expanded on each LLM request (e.g. in `COMMUNICATION.md` after merge with legacy `SESSION_INJECT.md`).
pub struct SessionInjectVars<'a> {
    pub workspace_root: &'a str,
}

pub fn expand_agent_prompt_placeholders(template: &str, vars: &SessionInjectVars<'_>) -> String {
    template.replace("{{workspace_root}}", vars.workspace_root)
}

/// Per-round **system inject**: shared [`COMMUNICATION_PUBLIC.md`] (native tool calling,
/// web citations, skills, language, and cross-profile rules).
/// Per-agent `COMMUNICATION.md` is merged after and expanded via [`expand_agent_prompt_placeholders`].
pub fn rendered_communication_public_inject() -> Option<String> {
    let pub_ = communication_public_md().trim();
    (!pub_.is_empty()).then(|| pub_.to_string())
}

#[derive(Default)]
pub struct AgentRegistry {
    inner: RwLock<HashMap<String, Arc<dyn AgentExecutor>>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<A>(&self, agent: A)
    where
        A: AgentExecutor + 'static,
    {
        self.inner
            .write()
            .insert(agent.def().id.clone(), Arc::new(agent));
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn AgentExecutor>> {
        self.inner.read().get(id.trim()).cloned()
    }

    pub fn list(&self) -> Vec<AgentDef> {
        let mut agents: Vec<_> = self.inner.read().values().map(|a| a.def()).collect();
        agents.sort_by(|a, b| a.name.cmp(&b.name));
        agents
    }

    pub fn enabled_workers(&self) -> Vec<AgentDef> {
        self.list()
            .into_iter()
            .filter(|a| a.enabled && a.role == "worker")
            .collect()
    }

    pub fn reload_external(&self) -> Result<usize> {
        let agents = load_external_agents()?;
        let count = agents.len();
        for agent in agents {
            self.register(agent);
        }
        Ok(count)
    }
}

pub struct AgentOrchestrator;

/// Normalize lead manifest `allowAgents`: trim, sort, dedup.
pub fn normalize_allow_agents(raw: &[String]) -> Vec<String> {
    let mut out: Vec<String> = raw
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Optional system block: worker metadata for lead manifest `allowAgents` (used with `run_subagent`).
pub fn delegatable_sub_agents_system_block(
    registry: &AgentRegistry,
    allow_ids: &[String],
) -> Option<String> {
    let ids: Vec<&str> = allow_ids
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if ids.is_empty() {
        return None;
    }
    let mut lines = vec![
        "Delegatable sub-agents (tool: run_subagent). Only call ids listed here. Metadata only — not full agent prompts."
            .to_string(),
    ];
    for id in ids {
        let Some(exec) = registry.get(id) else {
            lines.push(format!(
                "- id: {id} (unknown or disabled; tool calls will fail)"
            ));
            continue;
        };
        let d = exec.def();
        if d.role == "supervisor" || !d.enabled {
            lines.push(format!(
                "- id: {} (not a delegatable worker; tool calls will fail)",
                d.id
            ));
            continue;
        }
        lines.push(format!(
            "- id: {}\n  name: {}\n  role: {}\n  profile: {:?}\n  description: {}",
            d.id, d.name, d.role, d.profile, d.description
        ));
    }
    Some(lines.join("\n\n"))
}

impl AgentOrchestrator {
    pub fn build_plan(
        agents: &AgentRegistry,
        skills: &SkillRegistry,
        tools: &ToolRegistry,
        enabled_skill_ids: &[String],
        agent_skill_overrides: &HashMap<String, Vec<String>>,
        mode: &str,
        lead_worker_id: Option<&str>,
    ) -> AgentPlan {
        let normalized_mode = match mode {
            AGENT_MODE_SUPERVISOR => AGENT_MODE_SUPERVISOR,
            _ => AGENT_MODE_SINGLE,
        };
        let default_agent = agents.get(DEFAULT_LEAD_AGENT_ID).or_else(|| {
            agents.get(DEFAULT_AGENT_ID).or_else(|| {
                agents
                    .enabled_workers()
                    .into_iter()
                    .next()
                    .map(static_agent)
            })
        });

        if normalized_mode == AGENT_MODE_SINGLE {
            let mut agent = default_agent
                .as_ref()
                .map(|a| a.def())
                .unwrap_or_else(default_agent_def);
            if let Some(raw) = lead_worker_id {
                let tid = raw.trim();
                if !tid.is_empty() {
                    if let Some(exec) = agents.get(tid) {
                        let d = exec.def();
                        if d.role == "worker" && d.enabled {
                            agent = d;
                        }
                    }
                }
            }
            let session_skill_ids =
                resolve_skill_ids(&agent, enabled_skill_ids, agent_skill_overrides);
            let (skill_prompts, session_tools) = skills.progressive_context(&session_skill_ids);
            let allowed_tool_names = resolve_tools(&agent.access_policy, &session_tools, tools);
            let lead_prompt = agents.get(&agent.id).map(|a| a.system_prompt());
            let mut system_prompts = vec![agent_prompt(&agent, lead_prompt)];
            let active_system_prompt = system_prompts[0].clone();
            system_prompts.extend(skill_prompts.clone());

            return AgentPlan {
                mode: normalized_mode.into(),
                lead_agent_id: agent.id.clone(),
                lead_agent_name: agent_ui::agent_display_label(&agent),
                system_prompts,
                active_def: agent.clone(),
                active_system_prompt,
                resolved_skill_ids: session_skill_ids,
                resolved_skill_prompts: skill_prompts,
                allowed_tool_names,
                allow_agents: normalize_allow_agents(&agent.allow_agents),
            };
        }

        let supervisor = agents.get(SUPERVISOR_AGENT_ID);
        let lead = supervisor
            .as_ref()
            .map(|a| a.def())
            .unwrap_or_else(supervisor_agent_def);
        let worker_agents = agents.enabled_workers();
        let (skill_prompts, session_tools) = skills.progressive_context(&[]);
        let mut allowed_tool_names = Vec::new();
        for agent in &worker_agents {
            for tool in resolve_tools(&agent.access_policy, &session_tools, tools) {
                if !allowed_tool_names.contains(&tool) {
                    allowed_tool_names.push(tool);
                }
            }
        }
        if allowed_tool_names.is_empty() {
            allowed_tool_names = resolve_tools(&lead.access_policy, &session_tools, tools);
        }

        let mut system_prompts = vec![supervisor_prompt(
            &lead,
            supervisor.as_ref().map(|a| a.system_prompt()),
            &worker_agents,
        )];
        let active_system_prompt = system_prompts[0].clone();
        system_prompts.extend(skill_prompts.clone());

        AgentPlan {
            mode: normalized_mode.into(),
            lead_agent_id: lead.id.clone(),
            lead_agent_name: agent_ui::agent_display_label(&lead),
            system_prompts,
            active_def: lead.clone(),
            active_system_prompt,
            resolved_skill_ids: Vec::new(),
            resolved_skill_prompts: skill_prompts,
            allowed_tool_names,
            allow_agents: normalize_allow_agents(&lead.allow_agents),
        }
    }

    pub fn list_agents(agents: &AgentRegistry) -> Vec<AgentDef> {
        agents.list()
    }
}

pub fn register_builtin_agents(registry: &AgentRegistry) {
    for bundle in BUILTIN_AGENT_BUNDLES {
        let communication = if bundle.id == "computer" {
            builtin_computer_communication()
        } else {
            bundle.communication.to_string()
        };
        match load_builtin_agent(bundle.id, bundle.manifest, &communication) {
            Ok(agent) => registry.register(agent),
            Err(err) => log::warn!("load builtin agent failed: {}: {err}", bundle.id),
        }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_worker_role() -> String {
    "worker".into()
}

fn default_agent_def() -> AgentDef {
    load_builtin_agent(
        BUILTIN_AGENT_BUNDLES[0].id,
        BUILTIN_AGENT_BUNDLES[0].manifest,
        BUILTIN_AGENT_BUNDLES[0].communication,
    )
    .map(|agent| agent.def)
    .unwrap_or_else(|_| AgentDef {
        id: DEFAULT_AGENT_ID.into(),
        name: "general-assistant".into(),
        description: "Handles general tasks, simple Q&A, summarization, and default fallback."
            .into(),
        role: "worker".into(),
        profile: AgentProfile::General,
        default_skill_ids: Vec::new(),
        skills_policy: SkillsPolicy::UserConfigurable,
        access_policy: AccessPolicy::default(),
        builtin: true,
        enabled: true,
        tool_names: Vec::new(),
        source: None,
        resource_files: Vec::new(),
        allow_agents: Vec::new(),
        config: HashMap::new(),
        ui: AgentUiConfig::default(),
    })
}

fn supervisor_agent_def() -> AgentDef {
    load_builtin_agent(
        BUILTIN_AGENT_BUNDLES[1].id,
        BUILTIN_AGENT_BUNDLES[1].manifest,
        BUILTIN_AGENT_BUNDLES[1].communication,
    )
    .map(|agent| agent.def)
    .unwrap_or_else(|_| AgentDef {
        id: SUPERVISOR_AGENT_ID.into(),
        name: "团队模式".into(),
        description:
            "Understands goals, decomposes work, selects worker agents, and merges answers.".into(),
        role: "supervisor".into(),
        profile: AgentProfile::Supervisor,
        default_skill_ids: Vec::new(),
        skills_policy: SkillsPolicy::Disabled,
        access_policy: AccessPolicy::default(),
        builtin: true,
        enabled: true,
        tool_names: Vec::new(),
        source: None,
        resource_files: Vec::new(),
        allow_agents: Vec::new(),
        config: HashMap::new(),
        ui: AgentUiConfig::default(),
    })
}

fn load_builtin_agent(id: &str, raw: &str, communication: &str) -> Result<BaseAgent> {
    let mut manifest = parse_agent_md(raw)?;
    manifest.body = match id {
        "coder" => coder::composed_system_body(),
        "explore" => explore::composed_system_body(),
        _ => manifest.body,
    };
    let mut agent = manifest_to_agent(manifest, None, communication)?;
    agent.def.builtin = true;
    agent.def.source = Some(format!("builtin://{id}"));
    Ok(agent)
}

pub fn load_external_agents() -> Result<Vec<BaseAgent>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for root in agent_roots()? {
        if !root.exists() {
            continue;
        }
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            match load_agent_from_dir(&path) {
                Ok(agent) if seen.insert(agent.def.id.clone()) => out.push(agent),
                Ok(_) => {}
                Err(err) => log::warn!("load external agent failed: {}: {err}", path.display()),
            }
        }
    }

    Ok(out)
}

fn agent_roots() -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        roots.push(cwd.join(AGENTS_DIR));
        roots.push(cwd.join(".agents").join(AGENTS_DIR));
    }
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".agents").join(AGENTS_DIR));
    }
    let data_dir = storage::app_data_dir()?.join(AGENTS_DIR);
    fs::create_dir_all(&data_dir)?;
    roots.push(data_dir);
    Ok(roots)
}

fn load_agent_from_dir(dir: &Path) -> Result<BaseAgent> {
    if !is_kebab_case_dir(dir) {
        return Err(anyhow!(
            "Agent 目录名必须使用 kebab-case: {}",
            dir.display()
        ));
    }

    let manifest_path = dir.join(AGENT_MANIFEST);
    if !manifest_path.exists() {
        return Err(anyhow!("未找到 {}", AGENT_MANIFEST));
    }

    let raw = fs::read_to_string(&manifest_path)?;
    let manifest = parse_agent_md(&raw)?;
    let mut communication = if manifest.id == "computer" {
        load_external_computer_communication(dir)?
    } else if dir.join(AGENT_COMMUNICATION).exists() {
        fs::read_to_string(dir.join(AGENT_COMMUNICATION))?
    } else {
        String::new()
    };
    let inject_path = dir.join(AGENT_SESSION_INJECT);
    if inject_path.exists() {
        let inj = fs::read_to_string(&inject_path)?;
        if !inj.trim().is_empty() {
            communication = merge_legacy_session_into_communication(&inj, communication.trim());
        }
    }
    manifest_to_agent(manifest, Some(dir), &communication)
}

/// External `computer` agent dir: prefer `prompts/tiers/primary/`, fall back to legacy root filenames.
fn load_external_computer_communication(dir: &Path) -> Result<String> {
    let comm_candidates = [
        dir.join("prompts/tiers/primary/communication.md"),
        dir.join("COMMUNICATION.md"),
        dir.join(AGENT_COMMUNICATION),
    ];
    let mut communication = String::new();
    for path in comm_candidates {
        if path.exists() {
            communication = fs::read_to_string(&path)?;
            break;
        }
    }
    Ok(communication)
}

fn merge_legacy_session_into_communication(session_md: &str, communication: &str) -> String {
    let s = session_md.trim();
    if communication.is_empty() {
        s.to_string()
    } else {
        format!("{s}\n\n---\n\n{communication}")
    }
}

/// Optional per-agent `COMMUNICATION.md` + `AGENT.md` body. [`communication_public_md`] is prepended per request in `chat_service`.
fn compose_system_prompt(agent_communication: &str, body: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mid = agent_communication.trim();
    let b = body.trim();
    if !mid.is_empty() {
        parts.push(mid);
    }
    if !b.is_empty() {
        parts.push(b);
    }
    parts.join("\n\n---\n\n")
}

fn manifest_to_agent(
    manifest: AgentManifest,
    dir: Option<&Path>,
    communication: &str,
) -> Result<BaseAgent> {
    validate_agent_manifest(&manifest)?;
    let mut access_policy = manifest.access_policy;
    let tool_names = if manifest.tool_names.is_empty() {
        access_policy.allow_tools.clone()
    } else {
        manifest.tool_names
    };
    if access_policy.allow_tools.is_empty() && !tool_names.is_empty() {
        access_policy.allow_tools = tool_names.clone();
    }

    let def = AgentDef {
        id: manifest.id,
        name: manifest.name,
        description: manifest.description,
        role: manifest.role,
        profile: manifest
            .profile
            .unwrap_or_else(|| AgentProfile::Custom("external".into())),
        default_skill_ids: manifest.default_skill_ids,
        skills_policy: manifest.skills_policy,
        access_policy,
        builtin: false,
        enabled: manifest.enabled,
        tool_names,
        source: dir.map(|path| path.to_string_lossy().to_string()),
        resource_files: dir
            .map(collect_agent_resource_files)
            .transpose()?
            .unwrap_or_default(),
        allow_agents: normalize_allow_agents(&manifest.allow_agents),
        config: manifest.config,
        ui: manifest.ui,
    };

    Ok(BaseAgent {
        system_prompt: compose_system_prompt(communication, &manifest.body),
        def,
    })
}

fn parse_agent_md(raw: &str) -> Result<AgentManifest> {
    let text = raw.trim_start_matches('\u{feff}');
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err(anyhow!("AGENT.md 缺少 YAML frontmatter"));
    }

    let mut yaml = Vec::new();
    let mut body = Vec::new();
    let mut in_body = false;
    for line in lines {
        if !in_body && line.trim() == "---" {
            in_body = true;
            continue;
        }
        if in_body {
            body.push(line);
        } else {
            yaml.push(line);
        }
    }
    if !in_body {
        return Err(anyhow!("AGENT.md frontmatter 未闭合"));
    }

    let mut manifest: AgentManifest = serde_yaml::from_str(&yaml.join("\n"))?;
    manifest.body = body.join("\n").trim().to_string();
    Ok(manifest)
}

fn validate_agent_manifest(manifest: &AgentManifest) -> Result<()> {
    if !is_kebab_case(&manifest.id) {
        return Err(anyhow!("id 必须是 kebab-case"));
    }
    if manifest.name.trim().is_empty() {
        return Err(anyhow!("name 不能为空"));
    }
    if manifest.description.trim().is_empty() {
        return Err(anyhow!("description 不能为空"));
    }
    if manifest.role != "worker" && manifest.role != "supervisor" {
        return Err(anyhow!("role 只能是 worker 或 supervisor"));
    }
    if manifest.body.trim().is_empty() {
        return Err(anyhow!("AGENT.md 正文必须包含 system prompt"));
    }
    if manifest
        .access_policy
        .allow_tools
        .iter()
        .any(|tool| tool.trim().is_empty())
        || manifest
            .access_policy
            .deny_tools
            .iter()
            .any(|tool| tool.trim().is_empty())
        || manifest
            .access_policy
            .allow_skills
            .iter()
            .any(|skill| skill.trim().is_empty())
        || manifest
            .access_policy
            .deny_skills
            .iter()
            .any(|skill| skill.trim().is_empty())
        || manifest
            .tool_names
            .iter()
            .any(|tool| tool.trim().is_empty())
        || manifest
            .default_skill_ids
            .iter()
            .any(|skill| skill.trim().is_empty())
    {
        return Err(anyhow!("tools 或 skills 配置不允许包含空项"));
    }
    Ok(())
}

fn collect_agent_resource_files(dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for root_name in [
        "tools",
        "skills",
        "resources",
        "references",
        "assets",
        "scripts",
    ] {
        let root = dir.join(root_name);
        if root.exists() {
            collect_agent_resource_files_inner(dir, &root, &mut out)?;
        }
    }
    out.sort();
    Ok(out)
}

fn collect_agent_resource_files_inner(
    base: &Path,
    dir: &Path,
    out: &mut Vec<String>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_agent_resource_files_inner(base, &path, out)?;
        } else if path.is_file() {
            out.push(
                path.strip_prefix(base)?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    Ok(())
}

fn is_kebab_case_dir(dir: &Path) -> bool {
    dir.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_kebab_case)
}

fn is_kebab_case(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
        return false;
    }

    let mut prev_dash = false;
    for &byte in bytes {
        let valid = byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-';
        if !valid {
            return false;
        }
        if byte == b'-' {
            if prev_dash {
                return false;
            }
            prev_dash = true;
        } else {
            prev_dash = false;
        }
    }
    true
}

fn static_agent(def: AgentDef) -> Arc<dyn AgentExecutor> {
    Arc::new(BaseAgent {
        system_prompt: format!("You are {}. {}", def.name, def.description),
        def,
    })
}

/// Whether this agent can load any skills (including from overrides or defaults).
pub fn agent_supports_skills(agent: &AgentDef) -> bool {
    !matches!(agent.skills_policy, SkillsPolicy::Disabled)
}

/// Sub-agents that load session skills (**coder** uses its defaults).
pub fn sub_agent_inherits_session_skills(agent_id: &str) -> bool {
    matches!(agent_id, "coder")
}

/// Skill ids for a delegated sub-agent session. Uses `agent.skills_policy` to determine strategy.
pub fn sub_agent_skill_ids(
    agent: &AgentDef,
    parent_skill_ids: &[String],
    lead_agent_skill_overrides: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    resolve_skill_ids(agent, parent_skill_ids, lead_agent_skill_overrides)
}

/// Resolve effective skill ids:
/// `override[agent] ?? defaultSkillIds`, then merge missing bundled defaults.
///
/// `parent_skill_ids` is only used for [`SkillsPolicy::InheritsFromParent`] (lead's
/// already-resolved list). Legacy global `enabledSkillIds` is not a resolve source.
/// `accessPolicy.allowSkills` / `denySkills` are unused (kept for manifest compat).
fn resolve_skill_ids(
    agent: &AgentDef,
    parent_skill_ids: &[String],
    agent_skill_overrides: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let mut ids = match agent.skills_policy {
        SkillsPolicy::Disabled => return Vec::new(),
        SkillsPolicy::DefaultsOnly => agent.default_skill_ids.clone(),
        SkillsPolicy::UserConfigurable => {
            let mut ids = agent_skill_overrides
                .get(&agent.id)
                .or_else(|| agent_skill_overrides.get("_global"))
                .cloned()
                .unwrap_or_else(|| agent.default_skill_ids.clone());
            merge_missing_bundled_defaults(&agent.default_skill_ids, &mut ids);
            ids
        }
        SkillsPolicy::InheritsFromParent => {
            if parent_skill_ids.is_empty() {
                agent.default_skill_ids.clone()
            } else {
                parent_skill_ids.to_vec()
            }
        }
    };
    finalize_skill_ids(&mut ids);
    ids
}

/// Keep bundled skills listed in `defaultSkillIds` present after upgrades even when a
/// stale per-agent override was saved before those ids existed.
fn merge_missing_bundled_defaults(default_skill_ids: &[String], ids: &mut Vec<String>) {
    for id in default_skill_ids {
        if crate::skills::BUNDLED_SKILL_IDS
            .iter()
            .any(|bundled| *bundled == id.as_str())
            && !ids.iter().any(|existing| existing == id)
        {
            ids.push(id.clone());
        }
    }
}

fn finalize_skill_ids(ids: &mut Vec<String>) {
    // Skill boundary is defaultSkillIds + user override only.
    // accessPolicy.allowSkills / denySkills are legacy fields and are not applied.
    ids.sort();
    ids.dedup();
}

fn resolve_tools(
    policy: &AccessPolicy,
    session_tools: &[String],
    tools: &ToolRegistry,
) -> Vec<String> {
    let mut names = if policy.allow_tools.is_empty() {
        session_tools.to_vec()
    } else {
        policy.allow_tools.clone()
    };
    let available: HashSet<_> = tools.list_defs().into_iter().map(|t| t.name).collect();
    let deny: HashSet<_> = policy.deny_tools.iter().cloned().collect();
    names.retain(|name| {
        !deny.contains(name)
            && (available.contains(name.as_str())
                || available.iter().any(|reg| {
                    crate::tools::registry_tool_in_allow_list(std::slice::from_ref(name), reg)
                }))
    });
    names = crate::tools::expand_family_allow_names(&names, &available);
    crate::tools::normalize_allowed_tool_names(&mut names, &available);
    names
}

fn agent_prompt(agent: &AgentDef, prompt: Option<String>) -> String {
    format!(
        "Active agent:\n- id: {}\n- name: {}\n- role: {}\n- profile: {:?}\n- description: {}\n\n{}",
        agent.id,
        agent.name,
        agent.role,
        agent.profile,
        agent.description,
        prompt.unwrap_or_else(|| format!("You are {}. {}", agent.name, agent.description))
    )
}

fn supervisor_prompt(lead: &AgentDef, lead_prompt: Option<String>, agents: &[AgentDef]) -> String {
    let mut roster = String::new();
    for agent in agents {
        roster.push_str(&format!(
            "- id: {}\n  name: {}\n  role: {}\n  profile: {:?}\n  description: {}\n",
            agent.id, agent.name, agent.role, agent.profile, agent.description
        ));
        if !agent.tool_names.is_empty() {
            roster.push_str(&format!("  tools: {}\n", agent.tool_names.join(", ")));
        }
        if !agent.default_skill_ids.is_empty() {
            roster.push_str(&format!(
                "  skills: {}\n",
                agent.default_skill_ids.join(", ")
            ));
        }
    }

    format!(
        "{}\n\nYou operate in a multi-agent orchestration architecture.\n\nLead agent:\n- id: {}\n- name: {}\n- profile: {:?}\n- description: {}\n\nRoles:\n- Supervisor: understand the user goal, decompose work, pick worker agents by profile, merge their outputs.\n- Default agent: routine and unclassified fallback tasks.\n- Worker agents: execute subtasks per their profile; use tools when needed.\n- Reviewer/critic: check for gaps, conflicts, risk, and feasibility before final output.\n\nAvailable workers:\n{}\nProtocol:\n1. Decide whether multiple agents are needed; prefer the default agent or a single pass for simple work.\n2. For complex work, decompose explicitly and assign to the best-matching profile above.\n3. Tools and skills are shared pools; respect each agent's allow/deny policies.\n4. To use full skill text, call **`skill_read`**—do not invent skill details.\n5. Final replies should integrate conclusions only; briefly note which agents contributed when useful.",
        lead_prompt.unwrap_or_else(|| "You are the multi-agent Supervisor.".into()),
        lead.id,
        lead.name,
        lead.profile,
        lead.description,
        if roster.is_empty() {
            "- id: general\n  name: general-assistant\n  role: worker\n  profile: general\n  description: General-purpose fallback agent.\n".to_string()
        } else {
            roster
        }
    )
}

#[cfg(test)]
mod builtin_agent_tests {
    use super::*;

    #[test]
    fn resolve_skill_ids_uses_override_or_defaults_not_enabled_list() {
        let general = default_agent_def();
        // Legacy enabled list is ignored when overrides are empty.
        let ignored_enabled = vec!["my-skill".into()];
        let from_defaults = resolve_skill_ids(&general, &ignored_enabled, &HashMap::new());
        assert!(from_defaults.iter().any(|id| id == "skill-manager"));
        assert!(!from_defaults.iter().any(|id| id == "my-skill"));

        // Stale override missing a bundled default regains it via merge.
        let overrides = HashMap::from([(
            "general".to_string(),
            vec!["pdf".to_string(), "docx".to_string()],
        )]);
        let resolved = resolve_skill_ids(&general, &[], &overrides);
        assert!(resolved.iter().any(|id| id == "pdf"));
        assert!(resolved.iter().any(|id| id == "skill-manager"));
    }

    #[test]
    fn lead_plan_captures_effective_state_from_overrides() {
        let agents = AgentRegistry::new();
        register_builtin_agents(&agents);
        let skills = SkillRegistry::new();
        let tools = ToolRegistry::new();
        let store = std::sync::Arc::new(crate::task_board::TaskBoardStore::new());
        crate::tools::builtin::register_all(&tools, store);
        let overrides = HashMap::from([(
            "general".to_string(),
            vec!["captured-skill".to_string()],
        )]);
        let plan = AgentOrchestrator::build_plan(
            &agents,
            &skills,
            &tools,
            &[],
            &overrides,
            AGENT_MODE_SINGLE,
            Some("general"),
        );

        assert!(plan.resolved_skill_ids.iter().any(|id| id == "captured-skill"));
        assert!(plan.resolved_skill_ids.iter().any(|id| id == "skill-manager"));
        assert_eq!(
            plan.active_system_prompt,
            plan.system_prompts.first().cloned().unwrap()
        );
        assert_eq!(
            plan.resolved_skill_prompts,
            plan.system_prompts
                .iter()
                .skip(1)
                .cloned()
                .collect::<Vec<_>>()
        );
        assert_eq!(plan.active_def.id, plan.lead_agent_id);
    }

    #[test]
    fn resolve_tools_excludes_removed_response_tool() {
        let tools = crate::tools::ToolRegistry::new();
        let store = std::sync::Arc::new(crate::task_board::TaskBoardStore::new());
        crate::tools::builtin::register_all(&tools, store);
        let policy = AccessPolicy {
            allow_tools: vec!["terminal".into()],
            ..Default::default()
        };
        let names = resolve_tools(&policy, &["terminal".into()], &tools);
        assert!(
            !tools.list_defs().iter().any(|d| d.name == "response"),
            "response tool removed (OpenClaw-aligned: final reply is assistant content)"
        );
        assert!(
            !names.contains(&"response".to_string()),
            "response must not appear in resolved tool list"
        );
    }

    #[test]
    fn computer_resolve_tools_expands_captcha_family_to_flat_tools() {
        use crate::agents::computer::ComputerState;
        use crate::tools::builtin;
        use std::sync::Arc;

        let tools = crate::tools::ToolRegistry::new();
        let store = Arc::new(crate::task_board::TaskBoardStore::new());
        builtin::register_all(&tools, store);
        let computer_state = Arc::new(ComputerState::with_annotate_url("http://127.0.0.1:9"));
        builtin::register_computer_tools(&tools, computer_state);
        let raw = include_str!("computer/AGENT.md");
        let comm = builtin_computer_communication();
        let agent = load_builtin_agent("computer", raw, &comm).expect("load computer");
        let names = resolve_tools(&agent.def.access_policy, &[], &tools);
        assert!(
            names.iter().any(|n| n == "captcha_verify_click"),
            "expected flat captcha tools in allow list, got: {names:?}"
        );
        assert!(
            names.iter().any(|n| n == "captcha_verify_type"),
            "expected captcha_verify_type, got: {names:?}"
        );
        assert!(
            names.iter().any(|n| n == "captcha_verify_drag"),
            "expected captcha_verify_drag, got: {names:?}"
        );
        let captcha_only: Vec<String> = names
            .iter()
            .filter(|n| n.starts_with("captcha_verify_"))
            .cloned()
            .collect();
        let openai = tools.openai_tools(&captcha_only);
        assert_eq!(openai.len(), 3);
        assert!(
            openai
                .iter()
                .any(|t| t["function"]["name"] == "captcha_verify_click"),
            "captcha_verify_click should be exposed to the model"
        );
    }

    #[test]
    fn computer_builtin_manifest_parses_and_loads() {
        let raw = include_str!("computer/AGENT.md");
        let comm = builtin_computer_communication();
        let agent = load_builtin_agent("computer", raw, &comm).expect("load builtin computer");
        assert_eq!(agent.def.role, "worker");
        assert!(agent.def.enabled);
        assert_eq!(agent.def.profile, AgentProfile::Computer);
        assert!(
            agent.system_prompt.contains("Verify:"),
            "computer communication should merge Verify stage"
        );
        assert!(
            agent.system_prompt.contains("host runs **Verify**"),
            "computer communication should describe host verify"
        );
        assert!(
            agent.system_prompt.contains("[Annotated after action]"),
            "primary communication should reference annotated slot"
        );
        assert!(
            agent.system_prompt.contains("N–target relation"),
            "communication should require route decision"
        );
        assert!(
            agent.system_prompt.contains("Step 1 — Verify"),
            "communication should include Verify step"
        );
        assert!(
            agent
                .system_prompt
                .contains("Nearby overlay reference bboxes"),
            "communication should reference nearby bboxes"
        );
        assert!(
            agent.system_prompt.contains("Locate (Primary"),
            "communication should include Locate section"
        );
    }

    fn prompt_requires_top_level_trace_section(prompt: &str, heading: &str) -> bool {
        prompt.lines().any(|line| {
            let t = line.trim();
            t == heading || t.starts_with(&format!("{heading} —"))
        })
    }

    #[test]
    fn explore_builtin_manifest_parses_and_loads() {
        let raw = include_str!("explore/AGENT.md");
        let comm = include_str!("explore/COMMUNICATION.md");
        let agent = load_builtin_agent("explore", raw, comm).expect("load builtin explore");
        assert_eq!(agent.def.id, "explore");
        assert!(
            agent.def.description.contains("Use proactively"),
            "explore description should signal proactive delegation"
        );
        assert_eq!(agent.def.role, "worker");
        assert!(agent.def.enabled);
        assert_eq!(agent.def.profile, AgentProfile::Explore);
        let prompt = &agent.system_prompt;
        assert!(
            prompt.contains("Handoff contract"),
            "explore should include handoff contract"
        );
        assert!(
            prompt.contains("Execution paths (when mandatory)"),
            "explore should include trace_when rules"
        );
        assert!(
            !prompt_requires_top_level_trace_section(prompt, "## Forward trace"),
            "explore should not require top-level Forward trace section"
        );
        assert!(
            !prompt_requires_top_level_trace_section(prompt, "## Backward trace"),
            "explore should not require top-level Backward trace section"
        );
        assert!(
            prompt.contains("Scenario: cross_module_change"),
            "explore should include cross_module scenario playbook"
        );
        assert!(
            prompt.contains("Surfaces"),
            "explore cross-module playbook should mention Surfaces"
        );
    }

    #[test]
    fn communication_public_includes_instruction_priority() {
        let s = rendered_communication_public_inject().expect("public comm");
        assert!(
            s.contains("Instruction priority"),
            "COMMUNICATION_PUBLIC should define instruction priority stack"
        );
        assert!(
            s.contains("[USER RULES]"),
            "COMMUNICATION_PUBLIC priority stack should mention USER RULES"
        );
        assert!(
            s.contains("Workspace = this chat's scratch dir"),
            "COMMUNICATION_PUBLIC should define workspace purpose"
        );
        assert!(
            s.contains("MEDIA:"),
            "COMMUNICATION_PUBLIC should mention MEDIA delivery for workspace artifacts"
        );
    }

    #[test]
    fn coder_builtin_composed_prompt_anchors() {
        let raw = include_str!("coder/AGENT.md");
        let comm = include_str!("coder/COMMUNICATION.md");
        let agent = load_builtin_agent("coder", raw, comm).expect("load builtin coder");
        let prompt = &agent.system_prompt;
        assert!(prompt.contains("G1"), "coder should include G1 gate");
        assert!(prompt.contains("G2"), "coder should include G2 gate");
        assert!(prompt.contains("G3"), "coder should include G3 gate");
        assert!(
            prompt.contains("Scope gate"),
            "coder should include scope gate section"
        );
        assert!(
            prompt.contains("Related ≠ requested"),
            "coder should include related-vs-requested rule"
        );
        assert!(
            prompt.contains("When to delegate"),
            "coder should include explore delegation decision table"
        );
        assert!(
            prompt.contains("Hard stop"),
            "coder G2 should include read-loop hard stop"
        );
        assert!(
            prompt.contains("Delegating to the `explore` worker"),
            "coder should include delegation section"
        );
        assert!(
            prompt.contains("Parallel module exploration")
                && prompt.contains("calls in the **same turn**"),
            "coder should proactively split independent explore scopes into a parallel wave"
        );
        assert!(
            !prompt.contains("## Change impact scan"),
            "coder should not include legacy Change impact scan section"
        );
        assert!(
            !prompt.contains("Finding references and usages"),
            "coder should not include legacy Finding references section"
        );
        assert!(
            !prompt_requires_top_level_trace_section(prompt, "## Forward trace"),
            "coder should not include top-level Forward trace"
        );
        assert!(
            !prompt.contains("Exploration closure"),
            "coder should not duplicate explore impact_scan closure block"
        );
        assert!(
            prompt.contains("Scenario: production_debug"),
            "coder debugging scenario should mention production_debug"
        );
        assert!(
            prompt.contains("Scenario: skill_change"),
            "coder should include skill_change scenario playbook"
        );
        assert!(
            prompt.contains("G3 evidence gate"),
            "coder should include G3 evidence gate for same-turn verification"
        );
        assert!(
            prompt.contains("Unit test standards"),
            "coder should include explicit unit test standards in Check"
        );
        assert!(
            prompt.contains("When to add tests"),
            "coder should include when-to-add-tests guidance"
        );
    }

    #[test]
    fn coder_legacy_agent_archived_not_loaded() {
        let legacy = include_str!("coder/author/legacy_agent.md");
        assert!(
            legacy.contains("## Change impact scan"),
            "legacy archive should retain old section for A/B reference"
        );
        let raw = include_str!("coder/AGENT.md");
        let comm = include_str!("coder/COMMUNICATION.md");
        let agent = load_builtin_agent("coder", raw, comm).expect("load coder");
        assert!(
            !agent.system_prompt.contains("## Change impact scan"),
            "composed coder prompt must not load legacy body"
        );
    }

    #[test]
    fn explore_handoff_contract_includes_impact_map_surfaces() {
        let raw = include_str!("explore/AGENT.md");
        let comm = include_str!("explore/COMMUNICATION.md");
        let agent = load_builtin_agent("explore", raw, comm).expect("load explore");
        let prompt = &agent.system_prompt;
        assert!(
            prompt.contains("## Impact map"),
            "handoff contract should define Impact map section"
        );
        assert!(
            prompt.contains("Surfaces"),
            "handoff contract should mention Surfaces for cross-module work"
        );
    }

    #[test]
    fn communication_public_expands_workspace_placeholder() {
        let vars = SessionInjectVars {
            workspace_root: "/tmp/example-workspace",
        };
        let public = rendered_communication_public_inject().expect("public inject");
        let expanded = expand_agent_prompt_placeholders(&public, &vars);
        assert!(expanded.contains("/tmp/example-workspace"));
        assert!(!expanded.contains("{{workspace_root}}"));
    }

    #[test]
    fn media_delivery_inject_non_empty() {
        let block = rendered_media_delivery_inject().expect("media delivery inject");
        assert!(block.contains("Delivering local files in chat"));
        assert!(block.contains("MEDIA:"));
    }

    #[test]
    fn agent_def_json_includes_user_selectable() {
        let raw = include_str!("coder/AGENT.md");
        let comm = include_str!("coder/COMMUNICATION.md");
        let agent = load_builtin_agent("coder", raw, comm).expect("coder");
        assert_eq!(agent.def.ui.user_selectable, Some(true));
        let json = serde_json::to_string(&agent.def).expect("json");
        assert!(
            json.contains("userSelectable"),
            "json missing userSelectable: {}",
            json
        );
    }

    #[test]
    fn general_worker_not_registered_in_builtin_registry() {
        let reg = AgentRegistry::new();
        register_builtin_agents(&reg);
        assert!(
            reg.get("general-worker").is_none(),
            "general-worker must not be registered"
        );
    }

    #[test]
    fn general_builtin_allow_agents_excludes_general_worker() {
        let raw = include_str!("general/AGENT.md");
        let comm = include_str!("general/COMMUNICATION.md");
        let agent = load_builtin_agent("general", raw, comm).expect("load builtin general");
        assert!(
            agent
                .def
                .access_policy
                .allow_tools
                .contains(&"web_fetch".to_string()),
            "general should allow web_fetch"
        );
        assert!(
            agent
                .def
                .allow_agents
                .binary_search(&"general-worker".to_string())
                .is_err(),
            "general allowAgents must not include general-worker"
        );
        assert!(
            agent
                .def
                .allow_agents
                .binary_search(&"coder".to_string())
                .is_ok(),
            "general allowAgents should include coder"
        );
        assert!(
            agent
                .def
                .allow_agents
                .binary_search(&"computer".to_string())
                .is_ok(),
            "general allowAgents should include computer"
        );
    }

    #[test]
    fn general_builtin_allow_agents_includes_coder_and_computer() {
        let raw = include_str!("general/AGENT.md");
        let comm = include_str!("general/COMMUNICATION.md");
        let agent = load_builtin_agent("general", raw, comm).expect("load builtin general");
        for tool in [
            "file_read",
            "file_write",
            "file_edit",
            "file_grep",
            "file_glob",
            "file_list",
        ] {
            assert!(
                agent
                    .def
                    .access_policy
                    .allow_tools
                    .contains(&tool.to_string()),
                "general allowTools should include {tool}"
            );
        }
        for tool in ["skill_read", "skill_import"] {
            assert!(
                agent
                    .def
                    .access_policy
                    .allow_tools
                    .contains(&tool.to_string()),
                "general allowTools should keep {tool} (load skill stays local)"
            );
        }
        for skill in [
            "find-skills",
            "dev-env-setup",
            "skill-manager",
            "pointer-manager",
            "docx",
            "xlsx",
            "pptx",
            "pdf",
            "agent-browser",
        ] {
            assert!(
                agent.def.default_skill_ids.iter().any(|id| id == skill),
                "general defaultSkillIds should include {skill}"
            );
        }
    }

    #[test]
    fn sub_agent_inherits_session_skills_only_for_coder() {
        assert!(crate::agents::sub_agent_inherits_session_skills("coder"));
        assert!(!crate::agents::sub_agent_inherits_session_skills("explore"));
        assert!(!crate::agents::sub_agent_inherits_session_skills(
            "general-worker"
        ));
    }

    #[test]
    fn sub_agent_skill_ids_for_coder_uses_defaults_only() {
        let coder = load_builtin_agent(
            "coder",
            include_str!("coder/AGENT.md"),
            include_str!("coder/COMMUNICATION.md"),
        )
        .expect("load coder")
        .def;
        let ids = sub_agent_skill_ids(&coder, &["docx".into(), "pdf".into()], &HashMap::new());
        assert_eq!(
            ids,
            vec![
                "agent-browser".to_string(),
                "dev-env-setup".to_string(),
                "docx".to_string(),
                "find-skills".to_string(),
                "pdf".to_string(),
                "pptx".to_string(),
                "skill-manager".to_string(),
                "xlsx".to_string()
            ]
        );
    }
    #[test]
    fn default_lead_agent_id_is_general() {
        assert_eq!(DEFAULT_LEAD_AGENT_ID, "general");
    }

    #[test]
    fn coder_builtin_allow_agents_includes_explore() {
        let raw = include_str!("coder/AGENT.md");
        let comm = include_str!("coder/COMMUNICATION.md");
        let agent = load_builtin_agent("coder", raw, comm).expect("load builtin coder");
        assert!(
            agent
                .def
                .access_policy
                .allow_tools
                .contains(&"web_fetch".to_string()),
            "coder should allow web_fetch"
        );
        assert!(
            agent
                .def
                .allow_agents
                .binary_search(&"explore".to_string())
                .is_ok(),
            "coder allowAgents should include explore"
        );
        assert!(
            agent
                .def
                .default_skill_ids
                .iter()
                .any(|id| id == "skill-manager"),
            "coder defaultSkillIds should include skill-manager"
        );
        assert!(
            agent
                .def
                .default_skill_ids
                .iter()
                .any(|id| id == "agent-browser"),
            "coder defaultSkillIds should include agent-browser"
        );
        for tool in ["skill_read"] {
            assert!(
                agent
                    .def
                    .access_policy
                    .allow_tools
                    .contains(&tool.to_string()),
                "coder allowTools should include {tool}"
            );
        }
        assert!(
            !agent
                .def
                .access_policy
                .allow_tools
                .contains(&"skill_import".to_string()),
            "coder should not allow skill_import"
        );
    }

    #[test]
    fn resolve_skill_ids_uses_coder_defaults_only() {
        let coder = load_builtin_agent(
            "coder",
            include_str!("coder/AGENT.md"),
            include_str!("coder/COMMUNICATION.md"),
        )
        .expect("load coder")
        .def;
        assert!(agent_supports_skills(&coder));
        let ids = resolve_skill_ids(&coder, &[], &HashMap::new());
        assert_eq!(
            ids,
            vec![
                "agent-browser".to_string(),
                "dev-env-setup".to_string(),
                "docx".to_string(),
                "find-skills".to_string(),
                "pdf".to_string(),
                "pptx".to_string(),
                "skill-manager".to_string(),
                "xlsx".to_string()
            ]
        );
        // Parent/enabled list is ignored for UserConfigurable leads.
        let ids = resolve_skill_ids(&coder, &["docx".into()], &HashMap::new());
        assert_eq!(ids.len(), 8);
        assert!(ids.iter().any(|id| id == "skill-manager"));
    }

    #[test]
    fn research_builtin_manifest_parses_and_loads() {
        let raw = include_str!("research/AGENT.md");
        let comm = include_str!("research/COMMUNICATION.md");
        let agent = load_builtin_agent("research", raw, comm).expect("load builtin research");
        assert_eq!(agent.def.id, "research");
        assert_eq!(agent.def.name, "深度研究");
        assert_eq!(agent.def.profile, AgentProfile::Analyst);
        assert_eq!(agent.def.ui.show_sub_agent_trace, Some(true));
        assert_eq!(agent.def.ui.user_selectable, Some(false));
        assert_eq!(agent.def.ui.show_in_composer, Some(false));
        assert!(
            agent
                .def
                .access_policy
                .allow_tools
                .contains(&"web_search".to_string()),
            "research should allow web_search"
        );
    }
}
