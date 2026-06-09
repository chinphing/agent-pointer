use crate::session::build_conversation_key;
use crate::traits::{InboundMediaRef, InboundMessage, InboundReplyContext};
use serde_json::Value;

struct ParsedContent {
    text: String,
    attachments: Vec<InboundMediaRef>,
}

pub fn parse_inbound(event: &Value, account_id: &str) -> Option<InboundMessage> {
    let msgtype = event.get("msgtype").and_then(|v| v.as_str()).unwrap_or("");
    let parsed = parse_message_content(event, msgtype)?;
    if parsed.text.trim().is_empty() && parsed.attachments.is_empty() {
        return None;
    }

    let message_id = event
        .get("msgId")
        .and_then(|v| v.as_str())
        .or_else(|| event.get("msgid").and_then(|v| v.as_str()))
        .unwrap_or("unknown")
        .to_string();
    let sender_id = event
        .get("senderStaffId")
        .or_else(|| event.get("senderId"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let conversation_id = event
        .get("conversationId")
        .and_then(|v| v.as_str())
        .unwrap_or(&sender_id)
        .to_string();
    let conversation_type = event
        .get("conversationType")
        .and_then(|v| v.as_str())
        .unwrap_or("1");
    let is_group = conversation_type == "2";
    let session_webhook = event
        .get("sessionWebhook")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mentioned = event
        .get("isInAtList")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    Some(InboundMessage {
        channel: "dingtalk".into(),
        account_id: account_id.into(),
        message_id,
        conversation_key: build_conversation_key("dingtalk", &conversation_id, is_group),
        sender_id,
        sender_name: event
            .get("senderNick")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        text: parsed.text,
        is_group,
        mentioned_bot: mentioned,
        reply_context: Some(InboundReplyContext {
            session_webhook,
            chat_id: Some(conversation_id),
            open_id: None,
            context_token: None,
            wecom_req_id: None,
        }),
        attachments: parsed.attachments,
    })
}

fn parse_message_content(event: &Value, msgtype: &str) -> Option<ParsedContent> {
    match msgtype {
        "text" => {
            let text = event
                .get("text")
                .and_then(|t| t.get("content"))
                .and_then(|v| v.as_str())
                .or_else(|| event.get("content").and_then(|v| v.as_str()))?
                .to_string();
            Some(ParsedContent {
                text,
                attachments: vec![],
            })
        }
        "picture" => {
            let code = download_code_from(event, &["pictureDownloadCode", "downloadCode"])
                .or_else(|| event.get("content").and_then(download_code_from_value))?;
            let file_name = file_name_from(event, "picture");
            Some(ParsedContent {
                text: String::new(),
                attachments: vec![dingtalk_ref("image", code, file_name, Some("image/jpeg".into()))],
            })
        }
        "file" => {
            let code = download_code_from(event, &["fileDownloadCode", "downloadCode"])
                .or_else(|| event.get("content").and_then(download_code_from_value))?;
            let file_name = file_name_from(event, "file");
            Some(ParsedContent {
                text: String::new(),
                attachments: vec![dingtalk_ref("document", code, file_name, None)],
            })
        }
        "audio" => {
            let code = download_code_from(event, &["downloadCode"])
                .or_else(|| event.get("content").and_then(download_code_from_value))?;
            let file_name = file_name_from(event, "audio");
            Some(ParsedContent {
                text: String::new(),
                attachments: vec![dingtalk_ref("audio", code, file_name, None)],
            })
        }
        "video" => {
            let code = download_code_from(event, &["downloadCode", "videoDownloadCode"])
                .or_else(|| event.get("content").and_then(download_code_from_value))?;
            let file_name = file_name_from(event, "video");
            Some(ParsedContent {
                text: String::new(),
                attachments: vec![dingtalk_ref("video", code, file_name, Some("video/mp4".into()))],
            })
        }
        "richText" => parse_rich_text(event),
        _ => None,
    }
}

fn parse_rich_text(event: &Value) -> Option<ParsedContent> {
    let content = event.get("content")?;
    let rich = content
        .get("richText")
        .or_else(|| content.get("richTextList"))
        .and_then(|v| v.as_array())?;
    let mut text_parts = Vec::new();
    let mut attachments = Vec::new();
    for item in rich {
        let item_type = item
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match item_type.as_str() {
            "text" => {
                if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                    text_parts.push(t.to_string());
                }
            }
            "picture" | "image" => {
                if let Some(code) = download_code_from_value(item) {
                    attachments.push(dingtalk_ref(
                        "image",
                        code,
                        item.get("fileName")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        Some("image/jpeg".into()),
                    ));
                    text_parts.push("[image]".into());
                }
            }
            "video" => {
                if let Some(code) = download_code_from_value(item) {
                    attachments.push(dingtalk_ref(
                        "video",
                        code,
                        item.get("fileName")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        Some("video/mp4".into()),
                    ));
                    text_parts.push("[video]".into());
                }
            }
            "file" => {
                if let Some(code) = download_code_from_value(item) {
                    attachments.push(dingtalk_ref(
                        "document",
                        code,
                        item.get("fileName")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        None,
                    ));
                    text_parts.push("[file]".into());
                }
            }
            _ => {}
        }
    }
    Some(ParsedContent {
        text: text_parts.join("\n"),
        attachments,
    })
}

fn dingtalk_ref(
    kind: &str,
    download_code: String,
    file_name: Option<String>,
    mime_type: Option<String>,
) -> InboundMediaRef {
    InboundMediaRef {
        kind: kind.into(),
        mime_type,
        file_name,
        feishu_image_key: None,
        feishu_file_key: None,
        feishu_resource_type: None,
        wecom_download_url: None,
        wecom_aes_key: None,
        dingtalk_download_code: Some(download_code),
        weixin_encrypt_query_param: None,
        weixin_aes_key: None,
        weixin_image_aeskey_hex: None,
        weixin_voice_encode_type: None,
        weixin_voice_sample_rate: None,
        weixin_voice_asr_text: None,
    }
}

fn download_code_from(event: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(code) = event.get(*key).and_then(|v| v.as_str()) {
            let trimmed = code.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn download_code_from_value(value: &Value) -> Option<String> {
    download_code_from(
        value,
        &[
            "downloadCode",
            "pictureDownloadCode",
            "fileDownloadCode",
            "videoDownloadCode",
        ],
    )
}

fn file_name_from(event: &Value, msgtype: &str) -> Option<String> {
    event
        .get("content")
        .and_then(|c| c.get("fileName").or_else(|| c.get("filename")))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| event.get("fileName").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .or_else(|| {
            event
                .get(msgtype)
                .and_then(|o| o.get("fileName").or_else(|| o.get("filename")))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
}
