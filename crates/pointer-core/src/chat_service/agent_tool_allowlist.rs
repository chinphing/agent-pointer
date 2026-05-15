use crate::agents::AgentDef;
use crate::tools::ToolRegistry;

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
    names.retain(|name| {
        tools.get_def(name).is_some() && !agent.access_policy.deny_tools.contains(name)
    });

    if !agent.access_policy.deny_tools.iter().any(|d| d == "response")
        && tools.get_def("response").is_some()
        && !names.contains(&"response".into())
    {
        names.push("response".into());
    }

    names.sort();
    names.dedup();
    names
}
