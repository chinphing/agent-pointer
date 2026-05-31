//! Main-agent task board keying bound to user turns.

pub const MAIN_TURN_KEY_SEP: &str = "\u{1f}ptr_main_turn\u{1f}";

pub fn main_turn_task_board_store_key(conversation_id: &str, user_message_id: &str) -> String {
    format!(
        "{conv}{MAIN_TURN_KEY_SEP}{msg}",
        conv = conversation_id.trim(),
        msg = user_message_id.trim()
    )
}

pub fn is_main_turn_store_key(store_key: &str) -> bool {
    store_key.contains(MAIN_TURN_KEY_SEP)
}

pub fn conversation_id_from_main_turn_key(store_key: &str) -> Option<String> {
    let (conv, _) = store_key.split_once(MAIN_TURN_KEY_SEP)?;
    let conv = conv.trim();
    if conv.is_empty() {
        None
    } else {
        Some(conv.to_string())
    }
}

pub fn anchor_message_id_from_main_turn_key(store_key: &str) -> Option<String> {
    let (_, msg) = store_key.split_once(MAIN_TURN_KEY_SEP)?;
    let msg = msg.trim();
    if msg.is_empty() {
        None
    } else {
        Some(msg.to_string())
    }
}

pub fn looks_like_resume_intent(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    if t.is_empty() {
        return false;
    }
    let cn_keywords = [
        "继续",
        "接着",
        "接下来",
        "上次",
        "之前",
        "刚才",
        "延续",
        "补完",
        "继续执行",
        "继续做",
    ];
    if cn_keywords.iter().any(|k| t.contains(k)) {
        return true;
    }
    let en_keywords = [
        "continue",
        "resume",
        "pick up",
        "follow up",
        "carry on",
        "as above",
        "previous task",
    ];
    en_keywords.iter().any(|k| t.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_roundtrip_anchor() {
        let key = main_turn_task_board_store_key("conv-1", "msg-1");
        assert!(is_main_turn_store_key(&key));
        assert_eq!(
            conversation_id_from_main_turn_key(&key).as_deref(),
            Some("conv-1")
        );
        assert_eq!(anchor_message_id_from_main_turn_key(&key).as_deref(), Some("msg-1"));
    }

    #[test]
    fn resume_intent_detects_cn_and_en() {
        assert!(looks_like_resume_intent("继续上次任务"));
        assert!(looks_like_resume_intent("please continue this task"));
        assert!(!looks_like_resume_intent("新建一个独立任务"));
    }
}
