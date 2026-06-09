use crate::session::build_conversation_key;
use crate::traits::{InboundMediaRef, InboundMessage, InboundReplyContext};
use serde_json::Value;

struct ParsedContent {
    text: String,
    attachments: Vec<InboundMediaRef>,
}

pub fn parse_ws_inbound(body: &Value, account_id: &str, req_id: &str) -> Option<InboundMessage> {
    let msgtype = body.get("msgtype")?.as_str()?;
    let parsed = parse_message_content(body, msgtype)?;
    if parsed.text.trim().is_empty() && parsed.attachments.is_empty() {
        return None;
    }

    let message_id = body
        .get("msgid")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let sender_id = body.get("from")?.get("userid")?.as_str()?.to_string();
    let chat_id = body
        .get("chatid")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| sender_id.clone());
    let chattype = body.get("chattype").and_then(|v| v.as_str()).unwrap_or("single");
    let is_group = chattype == "group";

    Some(InboundMessage {
        channel: "wecom".into(),
        account_id: account_id.into(),
        message_id,
        conversation_key: build_conversation_key("wecom", &chat_id, is_group),
        sender_id: sender_id.clone(),
        sender_name: None,
        text: parsed.text,
        is_group,
        mentioned_bot: true,
        reply_context: Some(InboundReplyContext {
            session_webhook: None,
            chat_id: Some(chat_id),
            open_id: None,
            context_token: None,
            wecom_req_id: Some(req_id.to_string()),
        }),
        attachments: parsed.attachments,
    })
}

fn parse_message_content(body: &Value, msgtype: &str) -> Option<ParsedContent> {
    match msgtype {
        "text" => {
            let text = body
                .get("text")
                .and_then(|t| t.get("content"))
                .and_then(|v| v.as_str())?
                .to_string();
            Some(ParsedContent {
                text,
                attachments: vec![],
            })
        }
        "voice" => {
            let voice = body.get("voice")?;
            let text = voice
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let mut attachments = Vec::new();
            if let Some(att) = media_ref_from_url(voice, "audio", None) {
                attachments.push(att);
            }
            Some(ParsedContent { text, attachments })
        }
        "image" => {
            let image = body.get("image")?;
            let attachments = media_ref_from_url(image, "image", image.get("filename").and_then(|v| v.as_str().map(|s| s.to_string())))
                .into_iter()
                .collect();
            Some(ParsedContent {
                text: String::new(),
                attachments,
            })
        }
        "file" => {
            let file = body.get("file")?;
            let file_name = file.get("filename").and_then(|v| v.as_str()).map(|s| s.to_string());
            let attachments = media_ref_from_url(file, "document", file_name)
                .into_iter()
                .collect();
            Some(ParsedContent {
                text: String::new(),
                attachments,
            })
        }
        "video" => {
            let video = body.get("video")?;
            let file_name = video.get("filename").and_then(|v| v.as_str()).map(|s| s.to_string());
            let attachments = media_ref_from_url(video, "video", file_name)
                .into_iter()
                .collect();
            Some(ParsedContent {
                text: String::new(),
                attachments,
            })
        }
        "mixed" => parse_mixed(body),
        _ => None,
    }
}

fn parse_mixed(body: &Value) -> Option<ParsedContent> {
    let items = body.get("mixed")?.get("msg_item")?.as_array()?;
    let mut text_parts = Vec::new();
    let mut attachments = Vec::new();
    for item in items {
        let item_type = item.get("msgtype").and_then(|v| v.as_str()).unwrap_or("");
        match item_type {
            "text" => {
                if let Some(t) = item
                    .get("text")
                    .and_then(|x| x.get("content"))
                    .and_then(|v| v.as_str())
                {
                    text_parts.push(t.to_string());
                }
            }
            "image" => {
                if let Some(att) = item.get("image").and_then(|img| {
                    media_ref_from_url(img, "image", None)
                }) {
                    attachments.push(att);
                }
            }
            "file" => {
                if let Some(att) = item.get("file").and_then(|f| {
                    media_ref_from_url(
                        f,
                        "document",
                        f.get("filename").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    )
                }) {
                    attachments.push(att);
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

fn media_ref_from_url(
    obj: &Value,
    kind: &str,
    file_name: Option<String>,
) -> Option<InboundMediaRef> {
    let url = obj.get("url").and_then(|v| v.as_str()).filter(|s| !s.is_empty())?;
    let aes_key = obj
        .get("aeskey")
        .or_else(|| obj.get("aes_key"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?;
    Some(InboundMediaRef {
        kind: kind.into(),
        mime_type: None,
        file_name,
        feishu_image_key: None,
        feishu_file_key: None,
        feishu_resource_type: None,
        wecom_download_url: Some(url.to_string()),
        wecom_aes_key: Some(aes_key.to_string()),
        dingtalk_download_code: None,
        weixin_encrypt_query_param: None,
        weixin_aes_key: None,
        weixin_image_aeskey_hex: None,
        weixin_voice_encode_type: None,
        weixin_voice_sample_rate: None,
        weixin_voice_asr_text: None,
    })
}
