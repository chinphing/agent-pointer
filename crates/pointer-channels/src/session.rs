use crate::config::DynamicAgentsConfig;
use crate::traits::InboundMessage;

pub const SESSION_PEER_MAIN: &str = "_main";
pub const SESSION_PEER_GROUP: &str = "_group";

pub fn conversation_id(msg: &InboundMessage, dynamic: Option<&DynamicAgentsConfig>) -> String {
    let peer = resolve_session_peer(msg, dynamic);
    format!(
        "{}:{}:{}:{}",
        msg.channel, msg.account_id, msg.conversation_key, peer
    )
}

pub fn resolve_session_peer(msg: &InboundMessage, dynamic: Option<&DynamicAgentsConfig>) -> String {
    let Some(cfg) = dynamic.filter(|c| c.enabled) else {
        return msg.sender_id.clone();
    };

    if is_dynamic_admin(cfg, &msg.sender_id) {
        return SESSION_PEER_MAIN.to_string();
    }

    if msg.is_group {
        if cfg.group_enabled {
            return SESSION_PEER_GROUP.to_string();
        }
        return msg.sender_id.clone();
    }

    if cfg.dm_create_agent {
        return msg.sender_id.clone();
    }

    SESSION_PEER_MAIN.to_string()
}

pub fn is_dynamic_admin(cfg: &DynamicAgentsConfig, sender_id: &str) -> bool {
    let sender = sender_id.trim();
    cfg.admin_users
        .iter()
        .any(|admin| admin.trim().eq_ignore_ascii_case(sender))
}

/// Prefix group messages with sender when multiple users share one session.
pub fn should_prefix_group_sender(dynamic: &DynamicAgentsConfig, msg: &InboundMessage) -> bool {
    if !msg.is_group || !dynamic.enabled || !dynamic.group_enabled {
        return false;
    }
    !is_dynamic_admin(dynamic, &msg.sender_id)
}

pub fn format_group_sender_prefix(msg: &InboundMessage, text: &str) -> String {
    let label = msg
        .sender_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(msg.sender_id.as_str());
    format!("[{label}]: {text}")
}

pub fn build_conversation_key(channel: &str, chat_id: &str, is_group: bool) -> String {
    if is_group {
        format!("{channel}:group:{chat_id}")
    } else {
        format!("{channel}:dm:{chat_id}")
    }
}

/// Resolve the persisted `session_user_id` for an IM inbound message.
pub fn im_session_user_id(msg: &InboundMessage, dynamic: &DynamicAgentsConfig) -> String {
    if msg.is_group && dynamic.enabled && dynamic.group_enabled {
        return msg.conversation_key.clone();
    }
    msg.sender_id.clone()
}

/// Stable desktop user row id for one IM inbound platform message (retries upsert, no duplicate rows).
pub fn inbound_user_message_id(channel: &str, platform_message_id: &str) -> String {
    format!("ch-inbound:{}:{}", channel.trim(), platform_message_id.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::InboundMessage;

    fn sample_msg(is_group: bool, sender_id: &str, chat_id: &str) -> InboundMessage {
        InboundMessage {
            channel: "wecom".into(),
            account_id: "default".into(),
            message_id: "m1".into(),
            conversation_key: build_conversation_key("wecom", chat_id, is_group),
            sender_id: sender_id.into(),
            sender_name: None,
            text: "hi".into(),
            is_group,
            mentioned_bot: true,
            reply_context: None,
            attachments: vec![],
        }
    }

    fn dynamic_cfg() -> DynamicAgentsConfig {
        DynamicAgentsConfig {
            enabled: true,
            dm_create_agent: true,
            group_enabled: true,
            admin_users: vec!["admin1".into()],
        }
    }

    #[test]
    fn legacy_group_session_is_per_sender() {
        let msg = sample_msg(true, "user_a", "chat1");
        let id = conversation_id(&msg, None);
        assert!(id.ends_with(":user_a"));
    }

    #[test]
    fn dynamic_group_session_is_shared() {
        let msg = sample_msg(true, "user_a", "chat1");
        let cfg = dynamic_cfg();
        let id = conversation_id(&msg, Some(&cfg));
        assert!(id.ends_with(":_group"));
        let msg_b = sample_msg(true, "user_b", "chat1");
        assert_eq!(conversation_id(&msg, Some(&cfg)), conversation_id(&msg_b, Some(&cfg)));
    }

    #[test]
    fn dynamic_dm_session_is_per_user() {
        let cfg = dynamic_cfg();
        let a = conversation_id(&sample_msg(false, "u1", "u1"), Some(&cfg));
        let b = conversation_id(&sample_msg(false, "u2", "u2"), Some(&cfg));
        assert_ne!(a, b);
    }

    #[test]
    fn dynamic_admin_uses_main_session() {
        let cfg = dynamic_cfg();
        let admin = conversation_id(&sample_msg(true, "admin1", "chat1"), Some(&cfg));
        let dm_admin = conversation_id(&sample_msg(false, "admin1", "admin1"), Some(&cfg));
        assert!(admin.ends_with(":_main"));
        assert!(dm_admin.ends_with(":_main"));
    }

    #[test]
    fn inbound_user_message_id_is_stable() {
        let id = inbound_user_message_id("feishu", "om_abc123");
        assert_eq!(id, "ch-inbound:feishu:om_abc123");
    }

    #[test]
    fn im_session_user_id_group_uses_conversation_key() {
        let cfg = dynamic_cfg();
        let msg = sample_msg(true, "user_a", "chat1");
        assert_eq!(im_session_user_id(&msg, &cfg), "wecom:group:chat1");
    }

    #[test]
    fn im_session_user_id_dm_uses_sender() {
        let cfg = dynamic_cfg();
        let msg = sample_msg(false, "user_a", "user_a");
        assert_eq!(im_session_user_id(&msg, &cfg), "user_a");
    }
}
