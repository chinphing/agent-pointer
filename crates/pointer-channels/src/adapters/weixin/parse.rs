use crate::session::build_conversation_key;
use crate::traits::{InboundMediaRef, InboundMessage, InboundReplyContext};
use serde_json::Value;
use std::collections::HashMap;

const ITEM_TEXT: u64 = 1;
const ITEM_IMAGE: u64 = 2;
const ITEM_VOICE: u64 = 3;
const ITEM_FILE: u64 = 4;
const ITEM_VIDEO: u64 = 5;

struct ParsedContent {
    text: String,
    attachments: Vec<InboundMediaRef>,
}

pub fn parse_weixin_message(
    msg: &Value,
    account_id: &str,
    tokens: &mut HashMap<String, String>,
) -> Option<InboundMessage> {
    let msg_type = msg.get("message_type").and_then(json_as_u64);
    if let Some(t) = msg_type {
        if t != 1 {
            return None;
        }
    }
    let from = msg
        .get("from_user_id")
        .or_else(|| msg.get("from_user"))
        .or_else(|| msg.get("from"))
        .and_then(|v| v.as_str())?;
    if is_weixin_bot_sender(from) {
        log::debug!("weixin inbound drop self-sent from={from}");
        return None;
    }
    if let Some(token) = msg.get("context_token").and_then(|v| v.as_str()) {
        tokens.insert(from.to_string(), token.to_string());
    }
    let message_id = msg
        .get("message_id")
        .or_else(|| msg.get("msg_id"))
        .or_else(|| msg.get("id"))
        .map(json_value_to_id)
        .unwrap_or_else(|| "unknown".into());
    let items = msg
        .get("item_list")
        .or_else(|| msg.get("items"))
        .and_then(|v| v.as_array())?;
    let parsed = parse_item_list(items)?;
    if parsed.text.trim().is_empty() && parsed.attachments.is_empty() {
        return None;
    }
    let context_token = msg
        .get("context_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| tokens.get(from).cloned());
    Some(InboundMessage {
        channel: "weixin".into(),
        account_id: account_id.into(),
        message_id,
        conversation_key: build_conversation_key("weixin", from, false),
        sender_id: from.to_string(),
        sender_name: None,
        text: parsed.text,
        is_group: false,
        mentioned_bot: true,
        reply_context: Some(InboundReplyContext {
            session_webhook: None,
            chat_id: None,
            open_id: Some(from.to_string()),
            context_token,
            wecom_req_id: None,
        }),
        attachments: parsed.attachments,
    })
}

fn is_weixin_bot_sender(from: &str) -> bool {
    from.ends_with("@im.bot")
}

fn parse_item_list(items: &[Value]) -> Option<ParsedContent> {
    let mut text_parts: Vec<String> = Vec::new();
    let mut attachments: Vec<InboundMediaRef> = Vec::new();
    for item in items {
        parse_item(item, &mut text_parts, &mut attachments);
    }
    Some(ParsedContent {
        text: text_parts.join("\n"),
        attachments,
    })
}

fn parse_item(item: &Value, text_parts: &mut Vec<String>, attachments: &mut Vec<InboundMediaRef>) {
    let item_type = item_type_u64(item.get("type"));
    match item_type {
        Some(ITEM_TEXT) => {
            if let Some(t) = extract_text_item(item) {
                text_parts.push(t);
            }
        }
        Some(ITEM_IMAGE) => {
            if let Some(image) = item.get("image_item") {
                if let Some(att) = media_ref_from_cdn(
                    "image",
                    image.get("media"),
                    image.get("aeskey").and_then(|v| v.as_str()),
                    Some("image/jpeg"),
                    None,
                ) {
                    attachments.push(att);
                    if text_parts.last().map(|s| s.as_str()) != Some("[image]") {
                        text_parts.push("[image]".into());
                    }
                }
            }
        }
        Some(ITEM_VOICE) => {
            if let Some(voice) = item.get("voice_item") {
                if let Some(asr) = voice
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                {
                    text_parts.push(asr.to_string());
                }
                if let Some(mut att) = media_ref_from_cdn(
                    "audio",
                    voice.get("media"),
                    None,
                    None,
                    Some(format!("weixin-voice-{}.silk", uuid::Uuid::new_v4())),
                ) {
                    att.weixin_voice_encode_type = voice
                        .get("encode_type")
                        .and_then(json_as_u64)
                        .map(|v| v as u32);
                    att.weixin_voice_sample_rate = voice
                        .get("sample_rate")
                        .and_then(json_as_u64)
                        .map(|v| v as u32);
                    att.weixin_voice_asr_text = voice
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string());
                    attachments.push(att);
                }
            }
        }
        Some(ITEM_FILE) => {
            if let Some(file) = item.get("file_item") {
                let name = file
                    .get("file_name")
                    .or_else(|| file.get("filename"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                if let Some(att) = media_ref_from_cdn("document", file.get("media"), None, None, name) {
                    attachments.push(att);
                    text_parts.push("[file]".into());
                }
            }
        }
        Some(ITEM_VIDEO) => {
            if let Some(video) = item.get("video_item") {
                if let Some(att) = media_ref_from_cdn(
                    "video",
                    video.get("media"),
                    None,
                    Some("video/mp4"),
                    Some(format!("weixin-video-{}.mp4", uuid::Uuid::new_v4())),
                ) {
                    attachments.push(att);
                    text_parts.push("[video]".into());
                }
            }
        }
        _ => {}
    }
}

fn media_ref_from_cdn(
    kind: &str,
    media: Option<&Value>,
    image_aeskey_hex: Option<&str>,
    mime_type: Option<&str>,
    file_name: Option<String>,
) -> Option<InboundMediaRef> {
    let media = media?;
    let encrypt_query_param = media
        .get("encrypt_query_param")
        .or_else(|| media.get("encrypted_query_param"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())?;
    let aes_key = media
        .get("aes_key")
        .or_else(|| media.get("aeskey"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Some(InboundMediaRef::from_weixin_cdn(
        kind,
        encrypt_query_param.to_string(),
        aes_key,
        image_aeskey_hex
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty()),
        mime_type.map(|s| s.to_string()),
        file_name,
    ))
}

fn item_type_u64(v: Option<&Value>) -> Option<u64> {
    let v = v?;
    v.as_u64()
        .or_else(|| v.as_i64().and_then(|n| u64::try_from(n).ok()))
        .or_else(|| {
            v.as_str().and_then(|s| match s.to_ascii_lowercase().as_str() {
                "text" | "1" => Some(1),
                "image" | "2" => Some(2),
                "voice" | "3" => Some(3),
                "file" | "4" => Some(4),
                "video" | "5" => Some(5),
                _ => s.parse().ok(),
            })
        })
}

fn json_as_u64(v: &Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_i64().and_then(|n| u64::try_from(n).ok()))
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn json_value_to_id(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        s.to_string()
    } else if let Some(n) = v.as_i64() {
        n.to_string()
    } else if let Some(n) = v.as_u64() {
        n.to_string()
    } else {
        v.to_string()
    }
}

fn extract_text_item(it: &Value) -> Option<String> {
    it.get("text_item")
        .and_then(|t| t.get("text"))
        .or_else(|| it.get("text"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_text_message() {
        let mut tokens = HashMap::new();
        let inbound = parse_weixin_message(
            &json!({
                "message_id": 9812451782375u64,
                "from_user_id": "user@im.wechat",
                "message_type": 1,
                "context_token": "token-abc",
                "item_list": [{ "type": 1, "text_item": { "text": "你好" } }]
            }),
            "default",
            &mut tokens,
        )
        .expect("parse");
        assert_eq!(inbound.text, "你好");
        assert!(inbound.attachments.is_empty());
    }

    #[test]
    fn parse_voice_with_asr_and_attachment() {
        let mut tokens = HashMap::new();
        let inbound = parse_weixin_message(
            &json!({
                "message_id": "v1",
                "from_user_id": "user@im.wechat",
                "message_type": 1,
                "item_list": [{
                    "type": 3,
                    "voice_item": {
                        "text": "明天开会",
                        "encode_type": 6,
                        "sample_rate": 24000,
                        "media": {
                            "encrypt_query_param": "AAK2xQ8gqV9h5EmYxM2u3r7Q2A=",
                            "aes_key": "MDAxMTIyMzM0NDU1NjY3Nzg4OTlhYWJiY2NkZGVlZmY="
                        }
                    }
                }]
            }),
            "default",
            &mut tokens,
        )
        .expect("parse");
        assert_eq!(inbound.text, "明天开会");
        assert_eq!(inbound.attachments.len(), 1);
        assert_eq!(inbound.attachments[0].kind, "audio");
        assert_eq!(
            inbound.attachments[0].weixin_voice_asr_text.as_deref(),
            Some("明天开会")
        );
    }

    #[test]
    fn parse_image_attachment() {
        let mut tokens = HashMap::new();
        let inbound = parse_weixin_message(
            &json!({
                "message_id": "img1",
                "from_user_id": "user@im.wechat",
                "message_type": 1,
                "item_list": [{
                    "type": 2,
                    "image_item": {
                        "aeskey": "00112233445566778899aabbccddeeff",
                        "media": {
                            "encrypt_query_param": "AAFFc8c2PXQ5mKPw7rbcH7S1EA=",
                            "aes_key": "ABEiM0RVZneImaq7zN3u/w=="
                        }
                    }
                }]
            }),
            "default",
            &mut tokens,
        )
        .expect("parse");
        assert_eq!(inbound.attachments.len(), 1);
        assert_eq!(inbound.attachments[0].kind, "image");
    }

    #[test]
    fn drops_bot_sender_even_without_message_type() {
        let mut tokens = HashMap::new();
        let inbound = parse_weixin_message(
            &json!({
                "message_id": "bot1",
                "from_user_id": "hex@im.bot",
                "item_list": [{ "type": 1, "text_item": { "text": "echo" } }]
            }),
            "default",
            &mut tokens,
        );
        assert!(inbound.is_none());
    }
}
