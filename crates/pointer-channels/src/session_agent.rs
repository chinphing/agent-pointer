//! IM session agent / mode switch via agent display name or id.

use pointer_core::agents::{agent_display_label, AgentRegistry};
use pointer_core::channel_outbound::im_visible_workers;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSwitchTarget {
    pub lead_agent_id: Option<String>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentSwitchAction {
    SwitchOnly(AgentSwitchTarget),
    SwitchWithMessage(AgentSwitchTarget, String),
}

fn normalize_phrase(text: &str) -> String {
    text.trim()
        .trim_end_matches(['。', '！', '!', '？', '?', '.', ' '])
        .to_string()
}

struct AliasEntry {
    alias: String,
    target: AgentSwitchTarget,
}

fn push_aliases(entries: &mut Vec<AliasEntry>, lead: Option<String>, label: &str, id: &str, name: &str) {
    let target = AgentSwitchTarget {
        lead_agent_id: lead,
        label: label.to_string(),
    };
    for raw in [id, name, label] {
        let alias = raw.trim();
        if alias.is_empty() {
            continue;
        }
        entries.push(AliasEntry {
            alias: alias.to_string(),
            target: target.clone(),
        });
    }
}

fn collect_aliases(registry: &AgentRegistry) -> Vec<AliasEntry> {
    let mut entries = Vec::new();
    for def in im_visible_workers(registry) {
        push_aliases(
            &mut entries,
            Some(def.id.clone()),
            &agent_display_label(&def),
            &def.id,
            &def.name,
        );
    }
    entries.sort_by(|a, b| b.alias.len().cmp(&a.alias.len()));
    entries
}

pub fn detect_agent_switch(registry: &AgentRegistry, text: &str) -> Option<AgentSwitchAction> {
    let normalized = normalize_phrase(text);
    if normalized.is_empty() {
        return None;
    }
    let entries = collect_aliases(registry);
    let lower = normalized.to_ascii_lowercase();

    for entry in &entries {
        if lower == entry.alias.to_ascii_lowercase() {
            return Some(AgentSwitchAction::SwitchOnly(entry.target.clone()));
        }
    }

    for entry in &entries {
        let prefix = format!("{} ", entry.alias);
        let prefix_lower = prefix.to_ascii_lowercase();
        if lower.starts_with(&prefix_lower) {
            let rest = text[prefix.len()..].trim().to_string();
            return Some(if rest.is_empty() {
                AgentSwitchAction::SwitchOnly(entry.target.clone())
            } else {
                AgentSwitchAction::SwitchWithMessage(entry.target.clone(), rest)
            });
        }
    }
    None
}

pub fn agent_switch_ack(target: &AgentSwitchTarget) -> String {
    format!("已切换到{}。", target.label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pointer_core::agents::{register_builtin_agents, AgentRegistry};

    fn registry() -> AgentRegistry {
        let registry = AgentRegistry::new();
        register_builtin_agents(&registry);
        registry
    }

    #[test]
    fn switches_by_display_label() {
        let reg = registry();
        let action = detect_agent_switch(&reg, "电脑操控").unwrap();
        match action {
            AgentSwitchAction::SwitchOnly(t) => {
                assert_eq!(t.lead_agent_id.as_deref(), Some("computer"));
            }
            _ => panic!("expected switch only"),
        }
    }

    #[test]
    fn switches_with_follow_up_message() {
        let reg = registry();
        let action = detect_agent_switch(&reg, "通用助手 今天天气怎么样").unwrap();
        match action {
            AgentSwitchAction::SwitchWithMessage(t, rest) => {
                assert_eq!(t.lead_agent_id.as_deref(), Some("general"));
                assert_eq!(rest, "今天天气怎么样");
            }
            _ => panic!("expected switch with message"),
        }
    }

    #[test]
    fn ignores_normal_chat() {
        let reg = registry();
        assert!(detect_agent_switch(&reg, "帮我打开浏览器").is_none());
    }

    #[test]
    fn ignores_hidden_agents() {
        let reg = registry();
        assert!(detect_agent_switch(&reg, "团队模式").is_none());
        assert!(detect_agent_switch(&reg, "深度研究").is_none());
    }
}
