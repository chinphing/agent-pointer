use crate::adapters::feishu::auth::normalize_feishu_key;
use crate::session::build_conversation_key;
use crate::traits::{InboundMediaRef, InboundMessage, InboundReplyContext};
use serde_json::Value;

struct ParsedContent {
    text: String,
    attachments: Vec<InboundMediaRef>,
}

pub fn parse_feishu_event(root: &Value, account_id: &str) -> Option<InboundMessage> {
    let event_type = root
        .get("header")
        .and_then(|h| h.get("event_type"))
        .and_then(|v| v.as_str())
        .or_else(|| {
            root.get("event")
                .and_then(|e| e.get("type"))
                .and_then(|v| v.as_str())
        })?;
    if event_type != "im.message.receive_v1" {
        return None;
    }
    let ev = root.get("event")?;
    let message = ev.get("message")?;
    let sender = ev.get("sender")?;
    build_inbound_message(message, sender, account_id)
}

pub fn parse_feishu_message_event(event: &Value, account_id: &str) -> Option<InboundMessage> {
    let ev = event.get("event")?;
    let message = ev.get("message")?;
    let sender = ev.get("sender")?;
    build_inbound_message(message, sender, account_id)
}

fn build_inbound_message(
    message: &Value,
    sender: &Value,
    account_id: &str,
) -> Option<InboundMessage> {
    let message_id = message.get("message_id")?.as_str()?;
    let chat_id = message.get("chat_id")?.as_str()?;
    let chat_type = message
        .get("chat_type")
        .and_then(|v| v.as_str())
        .unwrap_or("p2p");
    let is_group = chat_type == "group" || chat_type == "topic_group";
    let message_type = message
        .get("message_type")
        .and_then(|v| v.as_str())
        .unwrap_or("text");
    let content_raw = message
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("{}");
    let parsed = parse_message_content(message_type, content_raw)?;
    if parsed.text.trim().is_empty() && parsed.attachments.is_empty() {
        return None;
    }
    let sender_id = sender
        .get("sender_id")
        .and_then(|s| s.get("open_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let mentioned_bot = message
        .get("mentions")
        .and_then(|v| v.as_array())
        .map(|m| !m.is_empty())
        .unwrap_or(false);

    Some(InboundMessage {
        channel: "feishu".into(),
        account_id: account_id.into(),
        message_id: message_id.into(),
        conversation_key: build_conversation_key("feishu", chat_id, is_group),
        sender_id,
        sender_name: None,
        text: parsed.text,
        is_group,
        mentioned_bot,
        reply_context: Some(InboundReplyContext {
            session_webhook: None,
            chat_id: Some(chat_id.into()),
            open_id: sender
                .get("sender_id")
                .and_then(|s| s.get("open_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            context_token: None,
            wecom_req_id: None,
        }),
        attachments: parsed.attachments,
    })
}

fn parse_message_content(message_type: &str, content_raw: &str) -> Option<ParsedContent> {
    match message_type {
        "text" => parse_text_content(content_raw),
        "image" => parse_image_content(content_raw),
        "file" | "audio" | "sticker" => parse_file_content(message_type, content_raw),
        "video" | "media" => parse_video_content(content_raw),
        "post" => parse_post_content(content_raw),
        _ => None,
    }
}

fn parse_text_content(content_raw: &str) -> Option<ParsedContent> {
    let content: Value = serde_json::from_str(content_raw).ok()?;
    let text = content.get("text")?.as_str()?.to_string();
    Some(ParsedContent {
        text,
        attachments: vec![],
    })
}

fn parse_image_content(content_raw: &str) -> Option<ParsedContent> {
    let content: Value = serde_json::from_str(content_raw).ok()?;
    let image_key = normalize_feishu_key(content.get("image_key").and_then(|v| v.as_str())?)?;
    Some(ParsedContent {
        text: String::new(),
        attachments: vec![InboundMediaRef {
            kind: "image".into(),
            mime_type: Some("image/jpeg".into()),
            file_name: content
                .get("file_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            feishu_image_key: Some(image_key),
            feishu_file_key: None,
            feishu_resource_type: Some("image".into()),
            wecom_download_url: None,
            wecom_aes_key: None,
            dingtalk_download_code: None,
            weixin_encrypt_query_param: None,
            weixin_aes_key: None,
            weixin_image_aeskey_hex: None,
            weixin_voice_encode_type: None,
            weixin_voice_sample_rate: None,
            weixin_voice_asr_text: None,
        }],
    })
}

fn parse_file_content(message_type: &str, content_raw: &str) -> Option<ParsedContent> {
    let content: Value = serde_json::from_str(content_raw).ok()?;
    let file_key = normalize_feishu_key(content.get("file_key").and_then(|v| v.as_str())?)?;
    let kind = match message_type {
        "audio" => "audio",
        "file" => "document",
        _ => "file",
    };
    Some(ParsedContent {
        text: String::new(),
        attachments: vec![InboundMediaRef {
            kind: kind.into(),
            mime_type: None,
            file_name: content
                .get("file_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            feishu_image_key: None,
            feishu_file_key: Some(file_key),
            feishu_resource_type: Some("file".into()),
            wecom_download_url: None,
            wecom_aes_key: None,
            dingtalk_download_code: None,
            weixin_encrypt_query_param: None,
            weixin_aes_key: None,
            weixin_image_aeskey_hex: None,
            weixin_voice_encode_type: None,
            weixin_voice_sample_rate: None,
            weixin_voice_asr_text: None,
        }],
    })
}

fn parse_video_content(content_raw: &str) -> Option<ParsedContent> {
    let content: Value = serde_json::from_str(content_raw).ok()?;
    let file_key = content
        .get("file_key")
        .and_then(|v| v.as_str())
        .and_then(normalize_feishu_key)
        .or_else(|| {
            content
                .get("image_key")
                .and_then(|v| v.as_str())
                .and_then(normalize_feishu_key)
        })?;
    Some(ParsedContent {
        text: String::new(),
        attachments: vec![InboundMediaRef {
            kind: "video".into(),
            mime_type: Some("video/mp4".into()),
            file_name: content
                .get("file_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            feishu_image_key: None,
            feishu_file_key: Some(file_key),
            feishu_resource_type: Some("file".into()),
            wecom_download_url: None,
            wecom_aes_key: None,
            dingtalk_download_code: None,
            weixin_encrypt_query_param: None,
            weixin_aes_key: None,
            weixin_image_aeskey_hex: None,
            weixin_voice_encode_type: None,
            weixin_voice_sample_rate: None,
            weixin_voice_asr_text: None,
        }],
    })
}

fn parse_post_content(content_raw: &str) -> Option<ParsedContent> {
    let parsed: Value = serde_json::from_str(content_raw).ok()?;
    let payload = resolve_post_payload(&parsed)?;
    let mut text_parts: Vec<String> = Vec::new();
    let mut attachments: Vec<InboundMediaRef> = Vec::new();

    for paragraph in payload.content {
        let Some(rows) = paragraph.as_array() else {
            continue;
        };
        let mut line = String::new();
        for element in rows {
            let Some(obj) = element.as_object() else {
                continue;
            };
            let tag = obj
                .get("tag")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            match tag.as_str() {
                "text" | "md" | "lark_md" => {
                    if let Some(t) = obj.get("text").and_then(|v| v.as_str()) {
                        line.push_str(t);
                    }
                }
                "a" => {
                    if let Some(t) = obj.get("text").and_then(|v| v.as_str()) {
                        line.push_str(t);
                    }
                }
                "at" => {
                    if let Some(t) = obj.get("user_name").and_then(|v| v.as_str()) {
                        line.push('@');
                        line.push_str(t);
                    }
                }
                "img" => {
                    if let Some(key) = obj
                        .get("image_key")
                        .and_then(|v| v.as_str())
                        .and_then(normalize_feishu_key)
                    {
                        attachments.push(InboundMediaRef {
                            kind: "image".into(),
                            mime_type: Some("image/jpeg".into()),
                            file_name: None,
                            feishu_image_key: Some(key),
                            feishu_file_key: None,
                            feishu_resource_type: Some("image".into()),
                            wecom_download_url: None,
                            wecom_aes_key: None,
                            dingtalk_download_code: None,
                            weixin_encrypt_query_param: None,
                            weixin_aes_key: None,
                            weixin_image_aeskey_hex: None,
                            weixin_voice_encode_type: None,
                            weixin_voice_sample_rate: None,
                            weixin_voice_asr_text: None,
                        });
                        line.push_str("![image]");
                    }
                }
                "media" => {
                    if let Some(key) = obj
                        .get("file_key")
                        .and_then(|v| v.as_str())
                        .and_then(normalize_feishu_key)
                    {
                        attachments.push(InboundMediaRef {
                            kind: "video".into(),
                            mime_type: None,
                            file_name: obj
                                .get("file_name")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            feishu_image_key: None,
                            feishu_file_key: Some(key),
                            feishu_resource_type: Some("file".into()),
                            wecom_download_url: None,
                            wecom_aes_key: None,
                            dingtalk_download_code: None,
                            weixin_encrypt_query_param: None,
                            weixin_aes_key: None,
                            weixin_image_aeskey_hex: None,
                            weixin_voice_encode_type: None,
                            weixin_voice_sample_rate: None,
                            weixin_voice_asr_text: None,
                        });
                        line.push_str("[media]");
                    }
                }
                "br" => line.push('\n'),
                _ => {}
            }
        }
        if !line.trim().is_empty() {
            text_parts.push(line);
        }
    }

    let title = payload.title.trim();
    let mut text = if title.is_empty() {
        text_parts.join("\n")
    } else {
        format!("{title}\n\n{}", text_parts.join("\n"))
    };
    if text.trim().is_empty() && attachments.is_empty() {
        text = "[Rich text message]".into();
    }
    Some(ParsedContent { text, attachments })
}

struct PostPayload {
    title: String,
    content: Vec<Value>,
}

fn resolve_post_payload(parsed: &Value) -> Option<PostPayload> {
    if let Some(p) = to_post_payload(parsed) {
        return Some(p);
    }
    if let Some(post) = parsed.get("post").and_then(to_post_payload) {
        return Some(post);
    }
    if let Some(obj) = parsed.as_object() {
        for value in obj.values() {
            if let Some(p) = to_post_payload(value) {
                return Some(p);
            }
        }
    }
    None
}

fn to_post_payload(value: &Value) -> Option<PostPayload> {
    let obj = value.as_object()?;
    let content = obj.get("content")?.as_array()?.clone();
    let title = obj
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some(PostPayload { title, content })
}
