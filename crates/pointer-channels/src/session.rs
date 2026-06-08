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
