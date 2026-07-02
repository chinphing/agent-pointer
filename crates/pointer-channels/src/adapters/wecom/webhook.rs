use async_trait::async_trait;
use serde_json::Value;

use crate::crypto::{wecom_decrypt, wecom_msg_signature};
use crate::session::build_conversation_key;
use crate::traits::{
    ChannelWebhookAdapter, InboundMediaRef, InboundMessage, InboundReplyContext, WebhookContext,
    WebhookResponse,
};

use super::media::WECOM_AGENT_MEDIA_PREFIX;
use super::mention::apply_wecom_mention;

pub struct WeComWebhook;

#[async_trait]
impl ChannelWebhookAdapter for WeComWebhook {
    fn channel_id(&self) -> crate::traits::ChannelId {
        "wecom"
    }

    async fn handle_webhook(&self, ctx: WebhookContext<'_>) -> anyhow::Result<WebhookResponse> {
        if ctx.method == "GET" {
            return Ok(self.handle_url_verify(ctx));
        }
        Ok(WebhookResponse {
            status: 200,
            content_type: "text/plain".into(),
            body: b"success".to_vec(),
        })
    }

    fn parse_inbound(&self, event: &Value, account_id: &str) -> Option<InboundMessage> {
        let cfg = crate::config::load_channels_config().ok()?;
        let account = cfg.account("wecom", account_id)?;
        let xml_text = if event.get("Encrypt").is_some() {
            let enc = event.get("Encrypt")?.as_str()?;
            wecom_decrypt(&account.encoding_aes_key, &account.corp_id, enc).ok()?
        } else {
            return None;
        };
        parse_wecom_xml(&xml_text, account_id)
    }
}

impl WeComWebhook {
    fn handle_url_verify(&self, ctx: WebhookContext<'_>) -> WebhookResponse {
        let query = ctx.query;
        let msg_signature = parse_query(query, "msg_signature");
        let timestamp = parse_query(query, "timestamp");
        let nonce = parse_query(query, "nonce");
        let echostr = parse_query(query, "echostr");
        let token = ctx.account.token.as_str();
        let sign = wecom_msg_signature(token, &timestamp, &nonce, &echostr);
        if sign != msg_signature {
            return WebhookResponse {
                status: 401,
                content_type: "text/plain".into(),
                body: b"invalid signature".to_vec(),
            };
        }
        let plain = wecom_decrypt(
            &ctx.account.encoding_aes_key,
            &ctx.account.corp_id,
            &echostr,
        )
        .unwrap_or(echostr);
        WebhookResponse {
            status: 200,
            content_type: "text/plain".into(),
            body: plain.into_bytes(),
        }
    }
}

fn parse_query(query: &str, key: &str) -> String {
    query
        .split('&')
        .find_map(|pair| {
            let mut it = pair.splitn(2, '=');
            let k = it.next()?;
            let v = it.next().unwrap_or("");
            if k == key {
                Some(urlencoding::decode(v).unwrap_or_else(|_| v.into()).to_string())
            } else {
                None
            }
        })
        .unwrap_or_default()
}

struct ParsedWebhookContent {
    text: String,
    attachments: Vec<InboundMediaRef>,
}

fn strip_cdata(value: &str) -> String {
    let v = value.trim();
    if let Some(inner) = v
        .strip_prefix("<![CDATA[")
        .and_then(|s| s.strip_suffix("]]>"))
    {
        inner.to_string()
    } else {
        v.to_string()
    }
}

fn parse_wecom_xml(xml: &str, account_id: &str) -> Option<InboundMessage> {
    let get_tag = |tag: &str| -> Option<String> {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let start = xml.find(&open)? + open.len();
        let end = xml.find(&close)?;
        let value = strip_cdata(xml[start..end].trim());
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    };
    let msg_type = get_tag("MsgType")?;
    let parsed = parse_wecom_xml_content(&msg_type, &get_tag)?;
    if parsed.text.trim().is_empty() && parsed.attachments.is_empty() {
        return None;
    }
    let from = get_tag("FromUserName")?;
    let msg_id = get_tag("MsgId").unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let chat_id = get_tag("ChatId").unwrap_or_else(|| from.clone());
    let is_group = get_tag("ChatId").is_some();
    let has_attachments = !parsed.attachments.is_empty();
    let mention = apply_wecom_mention(&parsed.text, is_group, false, has_attachments);

    Some(InboundMessage {
        channel: "wecom".into(),
        account_id: account_id.into(),
        message_id: msg_id,
        conversation_key: build_conversation_key("wecom", &chat_id, is_group),
        sender_id: from,
        sender_name: None,
        text: mention.text,
        is_group,
        mentioned_bot: mention.mentioned_bot,
        reply_context: Some(InboundReplyContext {
            session_webhook: None,
            chat_id: Some(chat_id),
            open_id: None,
            context_token: None,
            wecom_req_id: None,
        }),
        attachments: parsed.attachments,
    })
}

fn parse_wecom_xml_content(
    msg_type: &str,
    get_tag: &dyn Fn(&str) -> Option<String>,
) -> Option<ParsedWebhookContent> {
    match msg_type {
        "text" => {
            let text = get_tag("Content")?;
            Some(ParsedWebhookContent {
                text,
                attachments: vec![],
            })
        }
        "image" => {
            let media_id = get_tag("MediaId")?;
            let file_name = get_tag("PicUrl").map(|_| "image.jpg".into());
            Some(ParsedWebhookContent {
                text: String::new(),
                attachments: vec![agent_media_ref("image", &media_id, file_name, None)],
            })
        }
        "voice" => {
            let text = get_tag("Recognition").unwrap_or_default();
            let mut attachments = Vec::new();
            if let Some(media_id) = get_tag("MediaId") {
                attachments.push(agent_media_ref(
                    "audio",
                    &media_id,
                    Some("voice.amr".into()),
                    Some("audio/amr".into()),
                ));
            }
            Some(ParsedWebhookContent { text, attachments })
        }
        "video" => {
            let media_id = get_tag("MediaId")?;
            Some(ParsedWebhookContent {
                text: String::new(),
                attachments: vec![agent_media_ref(
                    "video",
                    &media_id,
                    Some("video.mp4".into()),
                    Some("video/mp4".into()),
                )],
            })
        }
        "file" => {
            let media_id = get_tag("MediaId")?;
            let file_name = get_tag("FileName").or_else(|| get_tag("Title"));
            Some(ParsedWebhookContent {
                text: String::new(),
                attachments: vec![agent_media_ref("document", &media_id, file_name, None)],
            })
        }
        _ => None,
    }
}

fn agent_media_ref(
    kind: &str,
    media_id: &str,
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
        wecom_download_url: Some(format!("{WECOM_AGENT_MEDIA_PREFIX}{media_id}")),
        wecom_aes_key: None,
        dingtalk_download_code: None,
        weixin_encrypt_query_param: None,
        weixin_aes_key: None,
        weixin_image_aeskey_hex: None,
        weixin_voice_encode_type: None,
        weixin_voice_sample_rate: None,
        weixin_voice_asr_text: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_agent_image_xml() {
        let xml = r#"<xml>
<ToUserName><![CDATA[corp]]></ToUserName>
<FromUserName><![CDATA[user1]]></FromUserName>
<MsgType><![CDATA[image]]></MsgType>
<MediaId><![CDATA[MEDIA_ID]]></MediaId>
<PicUrl><![CDATA[http://example.com/a.jpg]]></PicUrl>
<MsgId>123</MsgId>
</xml>"#;
        let msg = parse_wecom_xml(xml, "default").expect("parse");
        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].kind, "image");
        assert!(msg.attachments[0]
            .wecom_download_url
            .as_deref()
            .is_some_and(|u| u.contains("MEDIA_ID")));
    }

    #[test]
    fn webhook_group_text_requires_at_mention() {
        let xml = r#"<xml>
<ToUserName><![CDATA[corp]]></ToUserName>
<FromUserName><![CDATA[user1]]></FromUserName>
<ChatId><![CDATA[chat1]]></ChatId>
<MsgType><![CDATA[text]]></MsgType>
<Content><![CDATA[hello everyone]]></Content>
<MsgId>124</MsgId>
</xml>"#;
        let msg = parse_wecom_xml(xml, "default").expect("parse");
        assert!(!msg.mentioned_bot);
        assert_eq!(msg.text, "hello everyone");
    }

    #[test]
    fn webhook_group_text_strips_at_prefix() {
        let xml = r#"<xml>
<ToUserName><![CDATA[corp]]></ToUserName>
<FromUserName><![CDATA[user1]]></FromUserName>
<ChatId><![CDATA[chat1]]></ChatId>
<MsgType><![CDATA[text]]></MsgType>
<Content><![CDATA[@机器人 这是今日的测试情况]]></Content>
<MsgId>125</MsgId>
</xml>"#;
        let msg = parse_wecom_xml(xml, "default").expect("parse");
        assert!(msg.mentioned_bot);
        assert_eq!(msg.text, "这是今日的测试情况");
    }
}
