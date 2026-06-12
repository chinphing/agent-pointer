use crate::traits::InboundMessage;

pub fn conversation_id(msg: &InboundMessage) -> String {
    format!(
        "{}:{}:{}:{}",
        msg.channel, msg.account_id, msg.conversation_key, msg.sender_id
    )
}

pub fn build_conversation_key(channel: &str, chat_id: &str, is_group: bool) -> String {
    if is_group {
        format!("{channel}:group:{chat_id}")
    } else {
        format!("{channel}:dm:{chat_id}")
    }
}

/// Stable desktop user row id for one IM inbound platform message (retries upsert, no duplicate rows).
pub fn inbound_user_message_id(channel: &str, platform_message_id: &str) -> String {
    format!("ch-inbound:{}:{}", channel.trim(), platform_message_id.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbound_user_message_id_is_stable() {
        let id = inbound_user_message_id("feishu", "om_abc123");
        assert_eq!(id, "ch-inbound:feishu:om_abc123");
    }
}
