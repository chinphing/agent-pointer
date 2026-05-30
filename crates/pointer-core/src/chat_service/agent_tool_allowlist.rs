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
    let available: HashSet<_> = tools.list_defs().into_iter().map(|t| t.name).collect();
    let deny: HashSet<_> = agent.access_policy.deny_tools.iter().cloned().collect();
    names.retain(|name| {
        let base = crate::tools::registry_tool_base_name(name);
        available.contains(base) && !deny.contains(name) && !deny.contains(base)
    });
    normalize_allowed_tool_names(&mut names, &available);
    names
}
