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

/// Deep research agent runtime (SearchAgent web_search path, extension hooks).
pub mod research;

pub mod agent_ui;
pub use agent_ui::{resolve_agent_ui, AgentUiConfig, ResolvedAgentUi};

pub const AGENT_MODE_SINGLE: &str = "single";
pub const AGENT_MODE_SUPERVISOR: &str = "supervisor";
pub const DEFAULT_AGENT_ID: &str = "default";
/// Default worker selected in single-agent mode when `leadAgentId` is unset.
pub const DEFAULT_LEAD_AGENT_ID: &str = "computer";
pub const SUPERVISOR_AGENT_ID: &str = "supervisor";
const AGENTS_DIR: &str = "agents";
const AGENT_MANIFEST: &str = "AGENT.md";
const AGENT_COMMUNICATION: &str = "COMMUNICATION.md";
/// Legacy per-request block; if present, merged into `COMMUNICATION.md` content at load (placeholders expanded each request).
const AGENT_SESSION_INJECT: &str = "SESSION_INJECT.md";

/// Model-facing shared rules: host context, skills, **`thoughts`** meaning, and **`response`** role (English). XML shape and examples for **`response`** stay in the tools appendix.
const COMMUNICATION_PUBLIC: &str = include_str!("_shared/COMMUNICATION_PUBLIC.md");
/// Advanced tier: `[CUR_SCREEN]` image slots, frame registry, overlay digit rules.
const COMPUTER_VISION_SLOTS: &str =
    include_str!("computer/prompts/tiers/advanced/vision_slots.md");
const COMPUTER_COMMUNICATION_ADVANCED: &str =
    include_str!("computer/prompts/tiers/advanced/communication.md");
const COMPUTER_COMMUNICATION_PRIMARY: &str =
    include_str!("computer/prompts/tiers/primary/communication.md");
const COMPUTER_COMMUNICATION_INTERMEDIATE: &str =
    include_str!("computer/prompts/tiers/intermediate/communication.md");
const COMPUTER_AGENT_PRIMARY: &str = include_str!("computer/prompts/tiers/primary/loop.md");
const COMPUTER_AGENT_INTERMEDIATE: &str =
    include_str!("computer/prompts/tiers/intermediate/loop.md");
const COMPUTER_AGENT_ADVANCED: &str = include_str!("computer/prompts/tiers/advanced/loop.md");
const COMPUTER_OS_PROMPT_MACOS: &str = include_str!("computer/prompts/os/macos.md");
const COMPUTER_OS_PROMPT_WINDOWS: &str = include_str!("computer/prompts/os/windows.md");
const COMPUTER_OS_PROMPT_LINUX: &str = include_str!("computer/prompts/os/linux.md");

/// Injected on **every** main-LLM and sub-agent round (see `chat_service`).
pub fn communication_public_md() -> &'static str {
    COMMUNICATION_PUBLIC.trim()
}

struct BuiltinAgentBundle {
    id: &'static str,
    manifest: &'static str,
    communication: &'static str,
}

const BUILTIN_AGENT_BUNDLES: &[BuiltinAgentBundle] = &[
    BuiltinAgentBundle {
        id: "default",
        manifest: include_str!("default/AGENT.md"),
        communication: include_str!("default/COMMUNICATION.md"),
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
    computer_communication_for_tier(computer::tier::ComputerTier::Advanced)
}

/// Merged communication + OS slice for a computer tier (cacheable system prefix).
pub fn computer_communication_for_tier(tier: computer::tier::ComputerTier) -> String {
    let mut parts: Vec<&str> = Vec::new();
    match tier {
        computer::tier::ComputerTier::Primary => {
            push_trimmed(&mut parts, COMPUTER_COMMUNICATION_PRIMARY);
        }
        computer::tier::ComputerTier::Intermediate => {
            push_trimmed(&mut parts, COMPUTER_COMMUNICATION_INTERMEDIATE);
        }
        computer::tier::ComputerTier::Advanced => {
            push_trimmed(&mut parts, COMPUTER_VISION_SLOTS);
            push_trimmed(&mut parts, COMPUTER_COMMUNICATION_ADVANCED);
        }
    }
    push_trimmed(&mut parts, computer_os_prompt_md_for_platform());
    parts.join("\n\n---\n\n")
}

/// Agent loop body for a computer tier (merged into system prompt with communication).
pub fn computer_agent_body_for_tier(tier: computer::tier::ComputerTier) -> String {
    match tier {
        computer::tier::ComputerTier::Primary => COMPUTER_AGENT_PRIMARY.trim().to_string(),
        computer::tier::ComputerTier::Intermediate => COMPUTER_AGENT_INTERMEDIATE.trim().to_string(),
        computer::tier::ComputerTier::Advanced => COMPUTER_AGENT_ADVANCED.trim().to_string(),
    }
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
    #[serde(default, rename = "allowSkills")]
    pub allow_skills: Vec<String>,
    #[serde(default, rename = "denySkills")]
    pub deny_skills: Vec<String>,
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
    pub instruction: String,
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

/// Per-round **system inject**: shared [`COMMUNICATION_PUBLIC.md`] (`thoughts` on-wire semantics,
/// **`response`** usage, **`task_board`** + **`<sidecar_tools>`**).
/// Per-agent `COMMUNICATION.md` is merged after and expanded via [`expand_agent_prompt_placeholders`].
pub fn rendered_communication_public_inject() -> Option<String> {
    let pub_ = communication_public_md().trim();
    (!pub_.is_empty()).then(|| pub_.to_string())
}

const JSON_WIRE_TAIL: &str = include_str!("_shared/JSON_WIRE_TAIL.md");

/// Short **tail** system slice (after `[Environment]`) so JSON-only output stays in recent context.
pub fn rendered_json_wire_format_tail_inject() -> Option<String> {
    let t = JSON_WIRE_TAIL.trim();
    (!t.is_empty()).then(|| t.to_string())
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
        self.inner.read().get(id).cloned()
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
            lines.push(format!("- id: {id} (unknown or disabled; tool calls will fail)"));
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
            let session_skill_ids = resolve_skill_ids(&agent, enabled_skill_ids);
            let (skill_prompts, session_tools) = skills.progressive_context(&session_skill_ids);
            let allowed_tool_names = resolve_tools(&agent.access_policy, &session_tools, tools);
            let lead_prompt = agents
                .get(&agent.id)
                .map(|a| a.system_prompt());
            let mut system_prompts = vec![agent_prompt(&agent, lead_prompt)];
            system_prompts.extend(skill_prompts);

            return AgentPlan {
                mode: normalized_mode.into(),
                lead_agent_id: agent.id.clone(),
                lead_agent_name: agent.name.clone(),
                system_prompts,
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
        let skill_scope = enabled_skill_ids.to_vec();
        let (skill_prompts, session_tools) = skills.progressive_context(&skill_scope);
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
        system_prompts.extend(skill_prompts);

        AgentPlan {
            mode: normalized_mode.into(),
            lead_agent_id: lead.id.clone(),
            lead_agent_name: lead.name.clone(),
            system_prompts,
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
            name: "Default Agent".into(),
            description: "Handles general tasks, simple Q&A, summarization, and default fallback."
                .into(),
            role: "worker".into(),
            profile: AgentProfile::General,
            default_skill_ids: Vec::new(),
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
                "Understands goals, decomposes work, selects worker agents, and merges answers."
                    .into(),
            role: "supervisor".into(),
            profile: AgentProfile::Supervisor,
            default_skill_ids: Vec::new(),
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
    let manifest = parse_agent_md(raw)?;
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

/// External `computer` agent dir: prefer `prompts/tiers/advanced/`, fall back to legacy root filenames.
fn load_external_computer_communication(dir: &Path) -> Result<String> {
    let comm_candidates = [
        dir.join("prompts/tiers/advanced/communication.md"),
        dir.join("COMMUNICATION_ADVANCED.md"),
        dir.join(AGENT_COMMUNICATION),
    ];
    let mut communication = String::new();
    for path in comm_candidates {
        if path.exists() {
            communication = fs::read_to_string(&path)?;
            break;
        }
    }
    let shared_candidates = [
        dir.join("prompts/tiers/advanced/vision_slots.md"),
        dir.join("prompts/tiers/advanced/shared.md"),
        dir.join("COMMUNICATION_SHARED.md"),
    ];
    for path in shared_candidates {
        if path.exists() {
            let shared = fs::read_to_string(&path)?;
            communication = compose_system_prompt(shared.trim(), communication.trim());
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

/// Session skills come **only** from the caller’s `enabled_skill_ids`. Manifest `defaultSkillIds`
/// is metadata (e.g. UI hints / roster); it is not auto-merged into the session.
fn resolve_skill_ids(agent: &AgentDef, enabled_skill_ids: &[String]) -> Vec<String> {
    let mut ids = enabled_skill_ids.to_vec();
    if !agent.access_policy.allow_skills.is_empty() {
        let allow: HashSet<_> = agent.access_policy.allow_skills.iter().cloned().collect();
        ids.retain(|id| allow.contains(id));
    }
    let deny: HashSet<_> = agent.access_policy.deny_skills.iter().cloned().collect();
    ids.retain(|id| !deny.contains(id));
    ids.sort();
    ids.dedup();
    ids
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
    names.retain(|name| available.contains(name) && !deny.contains(name));
    // `response` is always available for every agent unless explicitly denied (final user-visible reply).
    if !deny.contains("response")
        && tools.get_def("response").is_some()
        && !names.contains(&"response".into())
    {
        names.push("response".into());
    }
    names.sort();
    names.dedup();
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
        "{}\n\nYou operate in a multi-agent orchestration architecture.\n\nLead agent:\n- id: {}\n- name: {}\n- profile: {:?}\n- description: {}\n\nRoles:\n- Supervisor: understand the user goal, decompose work, pick worker agents by profile, merge their outputs.\n- Default agent: routine and unclassified fallback tasks.\n- Worker agents: execute subtasks per their profile; use tools when needed.\n- Reviewer/critic: check for gaps, conflicts, risk, and feasibility before final output.\n\nAvailable workers:\n{}\nProtocol:\n1. Decide whether multiple agents are needed; prefer the default agent or a single pass for simple work.\n2. For complex work, decompose explicitly and assign to the best-matching profile above.\n3. Tools and skills are shared pools; respect each agent's allow/deny policies.\n4. To use full skill text, call the **skill** tool (`skill:load_instructions`)—do not invent skill details.\n5. Final replies should integrate conclusions only; briefly note which agents contributed when useful.",
        lead_prompt.unwrap_or_else(|| "You are the multi-agent Supervisor.".into()),
        lead.id,
        lead.name,
        lead.profile,
        lead.description,
        if roster.is_empty() {
            "- id: default\n  name: Default Agent\n  role: worker\n  profile: general\n  description: General-purpose fallback agent.\n".to_string()
        } else {
            roster
        }
    )
}

#[cfg(test)]
mod builtin_agent_tests {
    use super::*;

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
            agent.system_prompt.contains("Pointer:"),
            "computer communication should merge Pointer stage"
        );
        assert!(
            agent.system_prompt.contains("[Zoom pointer after action]"),
            "shared vision legend should be merged"
        );
        assert!(
            agent.system_prompt.contains("Recheck coordinates:"),
            "communication should require Recheck stage"
        );
        assert!(
            agent.system_prompt.contains("1 → 7"),
            "communication should require seven stages"
        );
        assert!(
            agent.system_prompt.contains("Visual facts"),
            "communication should include B2 visual fact discipline"
        );
        assert!(
            agent.system_prompt.contains("band="),
            "communication should use band|text|fill|size fields"
        );
        assert!(
            agent.system_prompt.contains("Diff:"),
            "communication should require Recheck R2 diff line"
        );
    }

    #[test]
    fn explore_builtin_manifest_parses_and_loads() {
        let raw = include_str!("explore/AGENT.md");
        let comm = include_str!("explore/COMMUNICATION.md");
        let agent = load_builtin_agent("explore", raw, comm).expect("load builtin explore");
        assert_eq!(agent.def.id, "explore");
        assert_eq!(agent.def.role, "worker");
        assert!(agent.def.enabled);
        assert_eq!(agent.def.profile, AgentProfile::Explore);
        assert!(
            agent.system_prompt.contains("How to explore workdir"),
            "explore body should include workdir playbook heading"
        );
    }

    #[test]
    fn coder_communication_expands_workspace_placeholder() {
        let vars = SessionInjectVars {
            workspace_root: "/tmp/example-workspace",
        };
        let public = rendered_communication_public_inject().expect("public inject");
        let expanded = expand_agent_prompt_placeholders(&public, &vars);
        assert!(expanded.contains("/tmp/example-workspace"));
        assert!(!expanded.contains("{{workspace_root}}"));
    }

    #[test]
    fn agent_def_json_includes_user_selectable() {
        let raw = include_str!("coder/AGENT.md");
        let comm = include_str!("coder/COMMUNICATION.md");
        let agent = load_builtin_agent("coder", raw, comm).expect("coder");
        assert_eq!(agent.def.ui.user_selectable, Some(true));
        let json = serde_json::to_string(&agent.def).expect("json");
        assert!(json.contains("userSelectable"), "json missing userSelectable: {}", json);
    }

    #[test]
    fn coder_builtin_allow_agents_includes_explore() {
        let raw = include_str!("coder/AGENT.md");
        let comm = include_str!("coder/COMMUNICATION.md");
        let agent = load_builtin_agent("coder", raw, comm).expect("load builtin coder");
        assert!(
            agent.def.allow_agents.binary_search(&"explore".to_string()).is_ok(),
            "coder allowAgents should include explore"
        );
        assert!(
            agent.def.allow_agents.binary_search(&"research".to_string()).is_ok(),
            "coder allowAgents should include research"
        );
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
        assert_eq!(agent.def.ui.user_selectable, Some(true));
        assert_eq!(agent.def.ui.show_in_composer, Some(true));
        assert!(
            agent.def.access_policy.allow_tools.contains(&"web_search".to_string()),
            "research should allow web_search"
        );
    }
}
