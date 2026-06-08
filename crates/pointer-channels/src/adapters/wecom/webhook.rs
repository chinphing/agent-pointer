use async_trait::async_trait;
use serde_json::Value;

use crate::crypto::{wecom_decrypt, wecom_msg_signature};
use crate::session::build_conversation_key;
use crate::traits::{
    ChannelWebhookAdapter, InboundMessage, InboundReplyContext, WebhookContext, WebhookResponse,
};

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

fn parse_wecom_xml(xml: &str, account_id: &str) -> Option<InboundMessage> {
    let get_tag = |tag: &str| -> Option<String> {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let start = xml.find(&open)? + open.len();
        let end = xml.find(&close)?;
        Some(xml[start..end].to_string())
    };
    let msg_type = get_tag("MsgType")?;
    if msg_type != "text" {
        return None;
    }
    let content = get_tag("Content")?;
    let from = get_tag("FromUserName")?;
    let msg_id = get_tag("MsgId").unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let chat_id = get_tag("ChatId").unwrap_or_else(|| from.clone());
    let is_group = get_tag("ChatId").is_some();

    Some(InboundMessage {
        channel: "wecom".into(),
        account_id: account_id.into(),
        message_id: msg_id,
        conversation_key: build_conversation_key("wecom", &chat_id, is_group),
        sender_id: from,
        sender_name: None,
        text: content,
        is_group,
        mentioned_bot: true,
        reply_context: Some(InboundReplyContext {
            session_webhook: None,
            chat_id: Some(chat_id),
            open_id: None,
            context_token: None,
            wecom_req_id: None,
        }),
    })
}
