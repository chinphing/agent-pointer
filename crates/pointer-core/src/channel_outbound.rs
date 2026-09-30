//! IM conversation helpers (titles, session commands, routing).

use std::path::Path;

use crate::agents::{AgentDef, AgentRegistry};
use crate::session_sandbox::SessionSandbox;

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
        if suffix.starts_with('s')
            && suffix.len() > 1
            && suffix[1..].chars().all(|c| c.is_ascii_digit())
        {
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

/// Workspace root to pass into `run_chat` for an IM desktop session.
///
/// Priority matches product docs: user-picked project folder → auto session sandbox.
/// Ignores polluted non-sandbox paths left by the pre-fix inherit bug.
pub fn resolve_im_run_workspace(stored_workspace: &str, workspace_user_set: bool) -> String {
    let trimmed = stored_workspace.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if workspace_user_set {
        return trimmed.to_string();
    }
    if SessionSandbox::is_sandbox(Path::new(trimmed)).unwrap_or(false) {
        return trimmed.to_string();
    }
    log::info!(
        "resolve_im_run_workspace: ignoring stored non-user workspace {trimmed}; will use session sandbox"
    );
    String::new()
}

/// System-prompt block listing IM-only user commands (reset, agent switch).
pub fn im_session_commands_block(registry: &AgentRegistry) -> String {
    let mut lines = vec![
        "[IM Channel Session]".to_string(),
        "The user is chatting through an IM integration (Feishu, DingTalk, WeCom, or Weixin)."
            .to_string(),
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

    let locale = crate::i18n::current_ui_locale();
    for def in im_visible_workers(registry) {
        let label = crate::agents::agent_display_label_for(&def, locale);
        lines.push(format!("- `{}` / `{}`", def.id, label));
    }

    lines.push(String::new());
    lines.push("## Outbound replies".to_string());
    lines.push(
        "Write your **final** reply in assistant message content; the host delivers it to the IM channel."
            .to_string(),
    );
    lines.push(
        "For attachments, end the reply with `MEDIA:<absolute-path>` (one file per line)."
            .to_string(),
    );
    lines.push("There is no separate delivery tool — do not expect a desktop chat UI.".to_string());

    lines.push(String::new());
    lines.push("## ask_user tool in IM".to_string());
    lines.push(
        "When you need the user to make a choice, call `ask_user` with the options.".to_string(),
    );
    lines.push(
        "In IM mode, `ask_user` **blocks this turn** until the user replies (or times out)."
            .to_string(),
    );
    lines.push(
        "Their next message is captured as the tool result — not a new conversation turn."
            .to_string(),
    );
    lines.push("Structure your response like this:".to_string());
    lines.push("1. Explain the situation to the user in assistant text.".to_string());
    lines.push(
        "2. List numbered options using the **exact** labels you will pass to `ask_user`."
            .to_string(),
    );
    lines.push(
        "3. Call `ask_user` with those options. Wait for the tool result (`selected`).".to_string(),
    );
    lines.push("4. Continue the task in this same turn using their choice.".to_string());
    lines.push(
        "Users may reply with `1` / `2`, the option label, or free text (Hermes-style)."
            .to_string(),
    );

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
        // The block and the expected labels are both rendered with
        // `current_ui_locale()`, which reads the app data dir from disk. Hold the
        // data-dir lock so a concurrent test cannot swap that dir between the two
        // reads (which flips the locale mid-test).
        let _data_dir_guard = crate::storage::test_app_data_dir_lock();
        let registry = crate::agents::AgentRegistry::new();
        crate::agents::register_builtin_agents(&registry);
        let block = im_session_commands_block(&registry);
        assert!(block.contains("/new"));
        assert!(block.contains("新对话"));
        assert!(block.contains("final"));
        assert!(!block.contains("channel_message"));
        assert!(block.contains("general"));
        assert!(block.contains("coder"));
        assert!(block.contains("computer"));
        let locale = crate::i18n::current_ui_locale();
        assert!(block.contains(&crate::i18n::t(locale, "agents.general")));
        assert!(block.contains(&crate::i18n::t(locale, "agents.coder")));
        assert!(block.contains(&crate::i18n::t(locale, "agents.computer")));
        assert!(!block.contains("research"));
        assert!(!block.contains(&crate::i18n::t(locale, "agents.analyst")));
        assert!(block.contains("blocks this turn"));
        assert!(block.contains("ask_user"));
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

    #[test]
    fn dingtalk_conversation_is_im() {
        let id = "dingtalk:default:dingtalk:dm:cid123:sender456";
        assert!(is_im_conversation(id));
        assert!(is_im_conversation(&format!("{id}@s1")));
    }

    #[test]
    fn resolve_im_run_workspace_honors_user_pick() {
        assert_eq!(
            resolve_im_run_workspace("/tmp/my-project", true),
            "/tmp/my-project"
        );
    }

    #[test]
    fn resolve_im_run_workspace_ignores_polluted_project_path() {
        assert_eq!(
            resolve_im_run_workspace("/Users/dev/pointer-app", false),
            ""
        );
    }
}
