use crate::agents::AgentDef;
use crate::tools::{normalize_allowed_tool_names, ToolRegistry};
use std::collections::HashSet;

pub(crate) fn resolve_agent_tools(
    agent: &AgentDef,
    session_tools: &[String],
    tools: &ToolRegistry,
) -> Vec<String> {
    let mut names = if agent.access_policy.allow_tools.is_empty() {
        session_tools.to_vec()
    } else {
        agent.access_policy.allow_tools.clone()
    };
    // 全局 MCP 工具默认对所有 agent 可见（用户主动添加的外部服务，注册即生效；
    // 可通过 denyTools 排除）。
    names.extend(tools.global_mcp_tool_names());
    names.sort();
    names.dedup();
    let available: HashSet<_> = tools.list_defs().into_iter().map(|t| t.name).collect();
    let deny: HashSet<_> = agent.access_policy.deny_tools.iter().cloned().collect();
    names.retain(|name| {
        !deny.contains(name)
            && (available.contains(name.as_str())
                || available.iter().any(|reg| {
                    crate::tools::registry_tool_in_allow_list(std::slice::from_ref(name), reg)
                }))
    });
    names = crate::tools::expand_family_allow_names(&names, &available);
    normalize_allowed_tool_names(&mut names, &available);
    names
}

pub(crate) fn retain_inheritable_subagent_tools(
    allowed_tools: &mut Vec<String>,
    registry: &ToolRegistry,
) {
    allowed_tools.retain(|name| registry.is_inheritable_to_subagent(name));
}
