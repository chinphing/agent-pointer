//! IM channel outbound bridge (installed by `pointer-channels` at startup).

use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

const IM_CHANNELS: &[&str] = &["feishu", "dingtalk", "wecom", "weixin"];
const DEFAULT_CONVERSATION_TITLE: &str = "新会话";

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

pub fn parse_im_conversation_parts(conversation_id: &str) -> Option<ImConversationParts> {
    if !is_im_conversation(conversation_id) {
        return None;
    }
    let parts: Vec<&str> = conversation_id.split(':').collect();
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
pub fn im_conversation_title(
    conversation_id: &str,
    sender_name: Option<&str>,
    first_user_text: Option<&str>,
) -> Option<String> {
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

#[derive(Debug, Clone)]
pub struct ChannelOutboundRequest {
    pub conversation_id: String,
    pub text: Option<String>,
    pub media_paths: Vec<String>,
}

pub type ChannelOutboundSender =
    Arc<dyn Fn(ChannelOutboundRequest) -> Result<()> + Send + Sync>;

static SENDER: OnceLock<ChannelOutboundSender> = OnceLock::new();
static ACTIVE_SESSIONS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashSet<String>> {
    ACTIVE_SESSIONS.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn is_im_conversation(conversation_id: &str) -> bool {
    let channel = conversation_id.split(':').next().unwrap_or("");
    IM_CHANNELS.contains(&channel)
}

pub fn set_sender(sender: ChannelOutboundSender) {
    let _ = SENDER.set(sender);
}

pub fn sender_configured() -> bool {
    SENDER.get().is_some()
}

pub fn register_im_session(conversation_id: &str) {
    if is_im_conversation(conversation_id) {
        sessions().lock().insert(conversation_id.to_string());
    }
}

pub fn unregister_im_session(conversation_id: &str) {
    sessions().lock().remove(conversation_id);
}

pub fn is_active_im_session(conversation_id: &str) -> bool {
    sessions().lock().contains(conversation_id)
}

pub fn send_channel_outbound(req: ChannelOutboundRequest) -> Result<()> {
    if !is_active_im_session(&req.conversation_id) {
        return Err(anyhow!(
            "channel_message: not an active IM session ({})",
            req.conversation_id
        ));
    }
    let sender = SENDER
        .get()
        .ok_or_else(|| anyhow!("channel outbound bridge not configured"))?;
    sender(req)
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
    fn im_title_group_fallback() {
        let id = "wecom:default:wecom:group:wr_group:userid";
        assert_eq!(
            im_conversation_title(id, None, None).as_deref(),
            Some("企微 群聊")
        );
    }
}
