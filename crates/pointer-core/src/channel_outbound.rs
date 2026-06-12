//! IM conversation helpers (titles, session commands, routing).

use crate::agents::{agent_display_label, AgentDef, AgentRegistry};

const IM_CHANNELS: &[&str] = &["feishu", "dingtalk", "wecom", "weixin"];
const DEFAULT_CONVERSATION_TITLE: &str = "新会话";

/// IM 会话内对用户暴露、可切换的智能体（通用助手 / 氛围编程 / 电脑操控）。
pub const IM_VISIBLE_AGENT_IDS: &[&str] = &["general", "coder", "computer"];

pub fn is_im_visible_agent_id(agent_id: &str) -> bool {
    IM_VISIBLE_AGENT_IDS
        .iter()
        .any(|id| id.eq_ignore_ascii_case(agent_id.trim()))
}

pub fn im_visible_workers(registry: &AgentRegistry) -> Vec<AgentDef> {
    registry
        .enabled_workers()
        .into_iter()
        .filter(|def| is_im_visible_agent_id(&def.id))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImConversationParts {
    pub channel: String,
    pub account_id: String,
    pub conversation_key: String,
    pub sender_id: String,
    pub is_group: bool,
}

fn channel_display_name(channel: &str) -> &str {
    match channel {
        "feishu" => "飞书",
        "dingtalk" => "钉钉",
        "wecom" => "企微",
        "weixin" => "微信",
        _ => channel,
    }
}

/// Strip fork suffix `@sN` from a desktop IM conversation id.
pub fn im_base_conversation_id(conversation_id: &str) -> String {
    if let Some((base, suffix)) = conversation_id.rsplit_once('@') {
        if suffix.starts_with('s') && suffix.len() > 1 && suffix[1..].chars().all(|c| c.is_ascii_digit()) {
            return base.to_string();
        }
    }
    conversation_id.to_string()
}

pub fn im_session_epoch(conversation_id: &str) -> u32 {
    if let Some((_, suffix)) = conversation_id.rsplit_once('@') {
        if let Some(rest) = suffix.strip_prefix('s') {
            return rest.parse().unwrap_or(0);
        }
    }
    0
}

pub fn im_desktop_conversation_id(base: &str, epoch: u32) -> String {
    if epoch == 0 {
        base.to_string()
    } else {
        format!("{base}@s{epoch}")
    }
}

pub fn parse_im_conversation_parts(conversation_id: &str) -> Option<ImConversationParts> {
    let base = im_base_conversation_id(conversation_id);
    if !is_im_conversation(&base) {
        return None;
    }
    let parts: Vec<&str> = base.split(':').collect();
    if parts.len() < 4 {
        return None;
    }
    let sender_id = parts.last()?.to_string();
    let conversation_key = parts[2..parts.len() - 1].join(":");
    let is_group = conversation_key.contains(":group:");
    Some(ImConversationParts {
        channel: parts[0].to_string(),
        account_id: parts[1].to_string(),
        conversation_key,
        sender_id,
        is_group,
    })
}

/// Human-readable sidebar title for an IM session.
/// Sidebar title for a forked IM desktop session (`@sN` suffix).
pub fn im_session_fork_title(
    conversation_id: &str,
    sender_name: Option<&str>,
    session_epoch: u32,
) -> Option<String> {
    let base = im_base_conversation_id(conversation_id);
    let base_title = im_conversation_title(&base, sender_name, None)?;
    if session_epoch == 0 {
        Some(base_title)
    } else {
        Some(format!("{base_title} · 新对话"))
    }
}

pub fn im_conversation_title(
    conversation_id: &str,
    sender_name: Option<&str>,
    first_user_text: Option<&str>,
) -> Option<String> {
    let epoch = im_session_epoch(conversation_id);
    if epoch > 0 && sender_name.is_none() && first_user_text.is_none() {
        return im_session_fork_title(conversation_id, None, epoch);
    }
    let parts = parse_im_conversation_parts(conversation_id)?;
    let label = channel_display_name(&parts.channel);
    if let Some(name) = sender_name.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(format!("{label} · {name}"));
    }
    if let Some(text) = first_user_text.map(str::trim).filter(|s| !s.is_empty()) {
        let preview: String = text.chars().take(24).collect();
        return Some(format!("{label} · {preview}"));
    }
    if parts.is_group {
        Some(format!("{label} 群聊"))
    } else {
        Some(format!("{label} 私信"))
    }
}

pub fn is_default_conversation_title(title: &str) -> bool {
    title.trim().is_empty() || title == DEFAULT_CONVERSATION_TITLE
}

pub fn is_im_conversation(conversation_id: &str) -> bool {
    let base = im_base_conversation_id(conversation_id);
    let channel = base.split(':').next().unwrap_or("");
    IM_CHANNELS.contains(&channel)
}

/// System-prompt block listing IM-only user commands (reset, agent switch).
pub fn im_session_commands_block(registry: &AgentRegistry) -> String {
    let mut lines = vec![
        "[IM Channel Session]".to_string(),
        "The user is chatting through an IM integration (Feishu, DingTalk, WeCom, or Weixin).".to_string(),
        "They may use the following commands without special syntax.".to_string(),
        String::new(),
        "## Reset conversation".to_string(),
        "- Send only: `/new`, `/reset`, `新对话`, or `重新开始`".to_string(),
        "- Or prefix: `/new <message>` or `/reset <message>` to reset then continue".to_string(),
        String::new(),
        "## Switch agent".to_string(),
        "- Send an agent id or display name alone to switch".to_string(),
        "- Or prefix: `<agent name> <question>` to switch then run the question".to_string(),
        "Available agents:".to_string(),
    ];

    for def in im_visible_workers(registry) {
        let label = agent_display_label(&def);
        lines.push(format!("- `{}` / `{}`", def.id, label));
    }

    lines.push(String::new());
    lines.push("## Outbound replies".to_string());
    lines.push(
        "Write your **final** reply in assistant message content; the host delivers it to the IM channel."
            .to_string(),
    );
    lines.push(
        "Append `MEDIA:` lines at the end for attachments (paths are not shown as raw text)."
            .to_string(),
    );
    lines.push("There is no separate delivery tool — do not expect a desktop chat UI.".to_string());

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn im_title_prefers_sender_name() {
        let id = "feishu:default:feishu:dm:oc_chat:ou_user";
        assert_eq!(
            im_conversation_title(id, Some("张三"), Some("hello")).as_deref(),
            Some("飞书 · 张三")
        );
    }

    #[test]
    fn im_title_uses_first_message_for_dm() {
        let id = "dingtalk:default:dingtalk:dm:cid123:sender456";
        assert_eq!(
            im_conversation_title(id, None, Some("帮我查一下天气")).as_deref(),
            Some("钉钉 · 帮我查一下天气")
        );
    }

    #[test]
    fn im_commands_block_lists_reset_and_visible_agents_only() {
        let registry = crate::agents::AgentRegistry::new();
        crate::agents::register_builtin_agents(&registry);
        let block = im_session_commands_block(&registry);
        assert!(block.contains("/new"));
        assert!(block.contains("新对话"));
        assert!(block.contains("final"));
        assert!(!block.contains("channel_message"));
        assert!(block.contains("general"));
        assert!(block.contains("通用助手"));
        assert!(block.contains("coder"));
        assert!(block.contains("氛围编程"));
        assert!(block.contains("computer"));
        assert!(block.contains("电脑操控"));
        assert!(!block.contains("supervisor"));
        assert!(!block.contains("团队模式"));
        assert!(!block.contains("research"));
        assert!(!block.contains("深度研究"));
    }

    #[test]
    fn im_title_group_fallback() {
        let id = "wecom:default:wecom:group:wr_group:userid";
        assert_eq!(
            im_conversation_title(id, None, None).as_deref(),
            Some("企微 群聊")
        );
    }

    #[test]
    fn im_fork_suffix_roundtrip() {
        let base = "feishu:default:feishu:dm:oc_chat:ou_user";
        let forked = im_desktop_conversation_id(base, 2);
        assert_eq!(forked, "feishu:default:feishu:dm:oc_chat:ou_user@s2");
        assert_eq!(im_base_conversation_id(&forked), base);
        assert_eq!(im_session_epoch(&forked), 2);
        assert!(parse_im_conversation_parts(&forked).is_some());
    }

    #[test]
    fn im_fork_title_marks_new_session() {
        let base = "feishu:default:feishu:dm:oc_chat:ou_user";
        let forked = im_desktop_conversation_id(base, 1);
        assert_eq!(
            im_session_fork_title(&forked, Some("张三"), 1).as_deref(),
            Some("飞书 · 张三 · 新对话")
        );
    }
}
