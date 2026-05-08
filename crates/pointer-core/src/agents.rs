use crate::skills::SkillRegistry;
use crate::storage;
use crate::tools::ToolRegistry;
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const AGENT_MODE_SINGLE: &str = "single";
pub const AGENT_MODE_SUPERVISOR: &str = "supervisor";
pub const DEFAULT_AGENT_ID: &str = "default";
pub const SUPERVISOR_AGENT_ID: &str = "supervisor";
const AGENTS_DIR: &str = "agents";
const AGENT_MANIFEST: &str = "AGENT.md";
const BUILTIN_AGENT_MANIFESTS: &[(&str, &str)] = &[
    ("default", include_str!("agents/default/AGENT.md")),
    ("supervisor", include_str!("agents/supervisor/AGENT.md")),
    ("coder", include_str!("agents/coder/AGENT.md")),
    ("reviewer", include_str!("agents/reviewer/AGENT.md")),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentProfile {
    General,
    Supervisor,
    Planner,
    Coder,
    Reviewer,
    Writer,
    Analyst,
    ToolUser,
    Custom(String),
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
}

#[derive(Debug, Clone)]
pub struct AgentPlan {
    pub mode: String,
    pub lead_agent_id: String,
    pub lead_agent_name: String,
    pub system_prompts: Vec<String>,
    pub allowed_tool_names: Vec<String>,
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

impl AgentOrchestrator {
    pub fn build_plan(
        agents: &AgentRegistry,
        skills: &SkillRegistry,
        tools: &ToolRegistry,
        enabled_skill_ids: &[String],
        mode: &str,
    ) -> AgentPlan {
        let normalized_mode = match mode {
            AGENT_MODE_SUPERVISOR => AGENT_MODE_SUPERVISOR,
            _ => AGENT_MODE_SINGLE,
        };
        let default_agent = agents
            .get(DEFAULT_AGENT_ID)
            .or_else(|| agents.enabled_workers().into_iter().next().map(static_agent));

        if normalized_mode == AGENT_MODE_SINGLE {
            let agent = default_agent
                .as_ref()
                .map(|a| a.def())
                .unwrap_or_else(default_agent_def);
            let session_skill_ids = resolve_skill_ids(&agent, enabled_skill_ids);
            let (skill_prompts, session_tools) = skills.progressive_context(&session_skill_ids);
            let allowed_tool_names = resolve_tools(&agent.access_policy, &session_tools, tools);
            let mut system_prompts = vec![agent_prompt(&agent, default_agent.as_ref().map(|a| a.system_prompt()))];
            system_prompts.extend(skill_prompts);

            return AgentPlan {
                mode: normalized_mode.into(),
                lead_agent_id: agent.id,
                lead_agent_name: agent.name,
                system_prompts,
                allowed_tool_names,
            };
        }

        let supervisor = agents.get(SUPERVISOR_AGENT_ID);
        let lead = supervisor
            .as_ref()
            .map(|a| a.def())
            .unwrap_or_else(supervisor_agent_def);
        let worker_agents = agents.enabled_workers();
        let mut skill_scope = enabled_skill_ids.to_vec();
        if skill_scope.is_empty() {
            skill_scope = default_skills_from_agents(&worker_agents);
        }
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
            lead_agent_id: lead.id,
            lead_agent_name: lead.name,
            system_prompts,
            allowed_tool_names,
        }
    }

    pub fn list_agents(agents: &AgentRegistry) -> Vec<AgentDef> {
        agents.list()
    }
}

pub fn register_builtin_agents(registry: &AgentRegistry) {
    for (id, raw) in BUILTIN_AGENT_MANIFESTS {
        match load_builtin_agent(id, raw) {
            Ok(agent) => registry.register(agent),
            Err(err) => log::warn!("load builtin agent failed: {id}: {err}"),
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
    load_builtin_agent(DEFAULT_AGENT_ID, BUILTIN_AGENT_MANIFESTS[0].1)
        .map(|agent| agent.def)
        .unwrap_or_else(|_| AgentDef {
            id: DEFAULT_AGENT_ID.into(),
            name: "Default Agent".into(),
            description: "负责常规任务、简单问答、总结和默认兜底处理。".into(),
            role: "worker".into(),
            profile: AgentProfile::General,
            default_skill_ids: vec!["general".into()],
            access_policy: AccessPolicy::default(),
            builtin: true,
            enabled: true,
            tool_names: Vec::new(),
            source: None,
            resource_files: Vec::new(),
        })
}

fn supervisor_agent_def() -> AgentDef {
    load_builtin_agent(SUPERVISOR_AGENT_ID, BUILTIN_AGENT_MANIFESTS[1].1)
        .map(|agent| agent.def)
        .unwrap_or_else(|_| AgentDef {
            id: SUPERVISOR_AGENT_ID.into(),
            name: "Supervisor".into(),
            description: "负责理解目标、拆解任务、选择子 Agent，并整合最终答案。".into(),
            role: "supervisor".into(),
            profile: AgentProfile::Supervisor,
            default_skill_ids: Vec::new(),
            access_policy: AccessPolicy::default(),
            builtin: true,
            enabled: true,
            tool_names: Vec::new(),
            source: None,
            resource_files: Vec::new(),
        })
}

fn load_builtin_agent(id: &str, raw: &str) -> Result<BaseAgent> {
    let manifest = parse_agent_md(raw)?;
    if manifest.id != id {
        return Err(anyhow!("内置 Agent id 与目录名不一致"));
    }
    let mut agent = manifest_to_agent(manifest, None)?;
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
        return Err(anyhow!("Agent 目录名必须使用 kebab-case: {}", dir.display()));
    }

    let manifest_path = dir.join(AGENT_MANIFEST);
    if !manifest_path.exists() {
        return Err(anyhow!("未找到 {}", AGENT_MANIFEST));
    }

    let raw = fs::read_to_string(&manifest_path)?;
    let manifest = parse_agent_md(&raw)?;
    let dir_name = dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("Agent 目录名无效"))?;
    if manifest.id != dir_name {
        return Err(anyhow!("Agent 目录名必须与 frontmatter id 一致"));
    }
    manifest_to_agent(manifest, Some(dir))
}

fn manifest_to_agent(manifest: AgentManifest, dir: Option<&Path>) -> Result<BaseAgent> {
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
        profile: manifest.profile.unwrap_or_else(|| AgentProfile::Custom("external".into())),
        default_skill_ids: manifest.default_skill_ids,
        access_policy,
        builtin: false,
        enabled: manifest.enabled,
        tool_names,
        source: dir.map(|path| path.to_string_lossy().to_string()),
        resource_files: dir.map(collect_agent_resource_files).transpose()?.unwrap_or_default(),
    };

    Ok(BaseAgent {
        system_prompt: manifest.body,
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
    if manifest.access_policy.allow_tools.iter().any(|tool| tool.trim().is_empty())
        || manifest.access_policy.deny_tools.iter().any(|tool| tool.trim().is_empty())
        || manifest.access_policy.allow_skills.iter().any(|skill| skill.trim().is_empty())
        || manifest.access_policy.deny_skills.iter().any(|skill| skill.trim().is_empty())
        || manifest.tool_names.iter().any(|tool| tool.trim().is_empty())
        || manifest.default_skill_ids.iter().any(|skill| skill.trim().is_empty())
    {
        return Err(anyhow!("tools 或 skills 配置不允许包含空项"));
    }
    Ok(())
}

fn collect_agent_resource_files(dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for root_name in ["tools", "skills", "resources", "references", "assets", "scripts"] {
        let root = dir.join(root_name);
        if root.exists() {
            collect_agent_resource_files_inner(dir, &root, &mut out)?;
        }
    }
    out.sort();
    Ok(out)
}

fn collect_agent_resource_files_inner(base: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_agent_resource_files_inner(base, &path, out)?;
        } else if path.is_file() {
            out.push(path.strip_prefix(base)?.to_string_lossy().replace('\\', "/"));
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
        system_prompt: format!("你是 {}。{}", def.name, def.description),
        def,
    })
}

fn resolve_skill_ids(agent: &AgentDef, enabled_skill_ids: &[String]) -> Vec<String> {
    let mut ids = if enabled_skill_ids.is_empty() {
        agent.default_skill_ids.clone()
    } else {
        enabled_skill_ids.to_vec()
    };
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

fn default_skills_from_agents(agents: &[AgentDef]) -> Vec<String> {
    let mut ids = Vec::new();
    for agent in agents {
        for id in &agent.default_skill_ids {
            if !ids.contains(id) && !agent.access_policy.deny_skills.contains(id) {
                ids.push(id.clone());
            }
        }
    }
    ids
}

fn resolve_tools(policy: &AccessPolicy, session_tools: &[String], tools: &ToolRegistry) -> Vec<String> {
    let mut names = if policy.allow_tools.is_empty() {
        session_tools.to_vec()
    } else {
        policy.allow_tools.clone()
    };
    let available: HashSet<_> = tools.list_defs().into_iter().map(|t| t.name).collect();
    let deny: HashSet<_> = policy.deny_tools.iter().cloned().collect();
    names.retain(|name| available.contains(name) && !deny.contains(name));
    names.sort();
    names.dedup();
    names
}

fn agent_prompt(agent: &AgentDef, prompt: Option<String>) -> String {
    format!(
        "当前执行 Agent：\n- id: {}\n- name: {}\n- role: {}\n- profile: {:?}\n- description: {}\n\n{}",
        agent.id,
        agent.name,
        agent.role,
        agent.profile,
        agent.description,
        prompt.unwrap_or_else(|| format!("你是 {}。{}", agent.name, agent.description))
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
            roster.push_str(&format!("  skills: {}\n", agent.default_skill_ids.join(", ")));
        }
    }

    format!(
        "{}\n\n你正在以多 Agent 编排架构工作。\n\n当前主控 Agent：\n- id: {}\n- name: {}\n- profile: {:?}\n- description: {}\n\n角色分工：\n- Supervisor：理解用户目标，拆解任务，基于 Agent profile 选择最合适的 Worker Agent，合并不同 Agent 的结论。\n- Default Agent：处理常规任务、简单任务和无法明确分类的兜底任务。\n- Worker Agent：根据自身 profile 与描述处理子任务；必要时调用可用工具。\n- Critic：在最终输出前检查遗漏、冲突、风险和可执行性。\n\n可用子 Agent：\n{}\n执行协议：\n1. 先判断任务是否需要多 Agent；常规任务优先交给 Default Agent 或直接完成。\n2. 复杂任务应显式拆解，并把子任务分配给上方最匹配 profile 的 Agent。\n3. 工具和 Skill 是共享能力池，但必须遵守每个 Agent 的 allow/deny 策略。\n4. 如需使用 Skill 的完整说明，先调用 load_skill_instructions，不要凭空假设细节。\n5. 最终答复只输出整合后的结论；必要时简要说明由哪些 Agent 参与。",
        lead_prompt.unwrap_or_else(|| "你是多 Agent Supervisor。".into()),
        lead.id,
        lead.name,
        lead.profile,
        lead.description,
        if roster.is_empty() {
            "- id: default\n  name: Default Agent\n  role: worker\n  profile: general\n  description: 常规任务处理 Agent。\n".to_string()
        } else {
            roster
        }
    )
}
