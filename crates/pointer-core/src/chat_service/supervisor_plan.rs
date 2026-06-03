//! Supervisor task planning: roster, JSON parse, fallback, and `chat_once` plan call.

use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::{anyhow, Result};
use tokio_util::sync::CancellationToken;

use crate::agents::{AgentDef, AgentRunLimits, AgentTask, DEFAULT_AGENT_ID};
use crate::llm_token_stats::ConversationLlmStats;
use crate::models::{ChatMessage, Role};
use crate::provider::OpenAIProvider;

use super::app_state::AppState;

pub(super) fn sort_agent_tasks_topologically(tasks: Vec<AgentTask>) -> Vec<AgentTask> {
    let n = tasks.len();
    if n <= 1 {
        return tasks;
    }
    let id_set: HashSet<_> = tasks.iter().map(|t| t.id.as_str()).collect();
    let mut indeg: HashMap<String, usize> = HashMap::new();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for t in &tasks {
        let c = t
            .depends_on
            .iter()
            .filter(|d| id_set.contains(d.as_str()))
            .count();
        indeg.insert(t.id.clone(), c);
    }
    for t in &tasks {
        for d in &t.depends_on {
            if id_set.contains(d.as_str()) {
                adj.entry(d.clone()).or_default().push(t.id.clone());
            }
        }
    }
    let mut q: VecDeque<String> = VecDeque::new();
    for t in &tasks {
        if indeg.get(&t.id).copied().unwrap_or(0) == 0 {
            q.push_back(t.id.clone());
        }
    }
    let mut order_ids = Vec::new();
    while let Some(u) = q.pop_front() {
        order_ids.push(u.clone());
        for v in adj.get(&u).into_iter().flatten() {
            let e = indeg.entry(v.clone()).or_insert(0);
            if *e > 0 {
                *e -= 1;
            }
            if *e == 0 {
                q.push_back(v.clone());
            }
        }
    }
    if order_ids.len() != n {
        log::warn!("agent task graph has cycle or inconsistent deps; using planner order");
        return tasks;
    }
    let mut by_id: HashMap<String, AgentTask> = tasks.into_iter().map(|t| (t.id.clone(), t)).collect();
    order_ids
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect()
}

fn agent_roster(agents: &[AgentDef]) -> String {
    agents
        .iter()
        .map(|agent| {
            format!(
                "- id: {}\n  name: {}\n  profile: {:?}\n  description: {}",
                agent.id, agent.name, agent.profile, agent.description
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_json_array(raw: &str) -> Option<String> {
    let start = raw.find('[')?;
    let end = raw.rfind(']')?;
    (start <= end).then(|| raw[start..=end].to_string())
}

fn parse_agent_tasks(
    raw: &str,
    workers: &[AgentDef],
    limits: &AgentRunLimits,
) -> Option<Vec<AgentTask>> {
    let json_text = extract_json_array(raw)?;
    let mut tasks: Vec<AgentTask> = serde_json::from_str(&json_text).ok()?;
    let allowed: Vec<_> = workers.iter().map(|agent| agent.id.as_str()).collect();
    tasks.retain(|task| allowed.contains(&task.agent_id.as_str()));
    for (idx, task) in tasks.iter_mut().enumerate() {
        if task.id.trim().is_empty() {
            task.id = format!("task_{}", idx + 1);
        }
        if task.title.trim().is_empty() {
            task.title = format!("Sub-task {}", idx + 1);
        }
    }
    tasks.truncate(limits.max_sub_agents);
    if tasks.is_empty() {
        None
    } else {
        Some(tasks)
    }
}

pub(super) fn fallback_agent_tasks(
    state: &AppState,
    history: &[ChatMessage],
    limits: &AgentRunLimits,
) -> Vec<AgentTask> {
    let latest = history
        .iter()
        .rev()
        .find(|message| matches!(message.role, Role::User))
        .map(|message| message.content.clone())
        .unwrap_or_else(|| "Fulfill the user request".into());
    let lower = latest.to_lowercase();
    let workers = state.agents.enabled_workers();
    let preferred = if lower.contains("代码")
        || lower.contains("实现")
        || lower.contains("code")
        || lower.contains("implement")
        || lower.contains("bug")
        || lower.contains("error")
        || lower.contains("rust")
        || lower.contains("vue")
    {
        "coder"
    } else if lower.contains("分析")
        || lower.contains("计算")
        || lower.contains("数据")
        || lower.contains("analy")
        || lower.contains("calculat")
        || lower.contains("data")
    {
        "analyst"
    } else if lower.contains("写")
        || lower.contains("文档")
        || lower.contains("总结")
        || lower.contains("write")
        || lower.contains("doc")
        || lower.contains("summar")
    {
        "writer"
    } else {
        DEFAULT_AGENT_ID
    };
    let agent_id = workers
        .iter()
        .find(|agent| agent.id == preferred)
        .or_else(|| workers.iter().find(|agent| agent.id == DEFAULT_AGENT_ID))
        .or_else(|| workers.first())
        .map(|agent| agent.id.clone())
        .unwrap_or_else(|| DEFAULT_AGENT_ID.into());
    vec![AgentTask {
        id: "task_1".into(),
        agent_id,
        title: "Handle user request".into(),
        instruction: latest,
        depends_on: Vec::new(),
    }]
    .into_iter()
    .take(limits.max_sub_agents)
    .collect()
}

pub(crate) async fn plan_agent_tasks(
    provider: &OpenAIProvider,
    state: &AppState,
    history: &[ChatMessage],
    limits: &AgentRunLimits,
    cancel: CancellationToken,
    env_context: &str,
    conversation_id: &str,
    assistant_message_id: &str,
    llm_stats: &mut ConversationLlmStats,
) -> Result<Vec<AgentTask>> {
    let workers = state.agents.enabled_workers();
    let roster = agent_roster(&workers);
    let prompt = format!(
        "{}\n\nYou are the Supervisor. Decompose the user's latest request into at most {} sub-agent tasks.\n\nPlanning rules: for work in one repo (implementation, debugging, refactor), **prefer a single** task with agentId `coder` and a complete `instruction`; split only when an independent reviewer pass or a clearly non-code subtask is needed. If multiple tasks have ordering, set `dependsOn` to an array of prerequisite task ids.\n\nAvailable agents:\n{}\n\nReturn **only** a JSON array (no Markdown). Element shape: {{\"id\":\"task_1\",\"agentId\":\"coder\",\"title\":\"short title\",\"instruction\":\"full instructions for that agent\",\"dependsOn\":[]}}. `agentId` must be from the list above. Use `general` for general Q&A; prefer `coder` for code, repo reads, and tests.",
        env_context,
        limits.max_sub_agents,
        roster
    );
    let dump_lbl = format!("{conversation_id}_{assistant_message_id}_supervisor_plan");
    let out = provider
        .chat_once(
            history,
            &crate::models::SystemPromptSections::all_cacheable(vec![prompt]),
            Vec::new(),
            cancel,
            None,
            Some(dump_lbl.as_str()),
        )
        .await?;
    let plan_scope =
        crate::agent_instance_scope::AgentInstanceScope::new(conversation_id, "supervisor");
    let model_name = crate::llm_token_stats::model_name_for_usage_report(&out.model);
    llm_stats.record_llm_round(&plan_scope, out.usage.as_ref(), model_name);
    parse_agent_tasks(&out.text, &workers, limits)
        .or_else(|| Some(fallback_agent_tasks(state, history, limits)))
        .ok_or_else(|| anyhow!("无法生成 Agent 任务计划"))
}
