use serde::{Deserialize, Serialize};

pub type ChannelId = &'static str;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundMediaRef {
    /// `image`, `document`, `audio`, `video`, `file`
    pub kind: String,
    #[serde(default, rename = "mimeType")]
    pub mime_type: Option<String>,
    #[serde(default, rename = "fileName")]
    pub file_name: Option<String>,
    #[serde(default, rename = "feishuImageKey")]
    pub feishu_image_key: Option<String>,
    #[serde(default, rename = "feishuFileKey")]
    pub feishu_file_key: Option<String>,
    /// Feishu message resource API `type`: `image`, `file`, or `media`.
    #[serde(default, rename = "feishuResourceType")]
    pub feishu_resource_type: Option<String>,
    #[serde(default, rename = "wecomDownloadUrl")]
    pub wecom_download_url: Option<String>,
    #[serde(default, rename = "wecomAesKey")]
    pub wecom_aes_key: Option<String>,
    #[serde(default, rename = "dingtalkDownloadCode")]
    pub dingtalk_download_code: Option<String>,
    #[serde(default, rename = "weixinEncryptQueryParam")]
    pub weixin_encrypt_query_param: Option<String>,
    #[serde(default, rename = "weixinAesKey")]
    pub weixin_aes_key: Option<String>,
    /// 32-char hex AES key on some inbound images (`image_item.aeskey`).
    #[serde(default, rename = "weixinImageAeskeyHex")]
    pub weixin_image_aeskey_hex: Option<String>,
    #[serde(default, rename = "weixinVoiceEncodeType")]
    pub weixin_voice_encode_type: Option<u32>,
    #[serde(default, rename = "weixinVoiceSampleRate")]
    pub weixin_voice_sample_rate: Option<u32>,
    /// WeChat ASR on voice messages; skip re-transcription when set.
    #[serde(default, rename = "weixinVoiceAsrText")]
    pub weixin_voice_asr_text: Option<String>,
}

impl InboundMediaRef {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            mime_type: None,
            file_name: None,
            feishu_image_key: None,
            feishu_file_key: None,
            feishu_resource_type: None,
            wecom_download_url: None,
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

    pub fn from_weixin_cdn(
        kind: impl Into<String>,
        encrypt_query_param: String,
        aes_key: Option<String>,
        image_aeskey_hex: Option<String>,
        mime_type: Option<String>,
        file_name: Option<String>,
    ) -> Self {
        Self {
            kind: kind.into(),
            mime_type,
            file_name,
            feishu_image_key: None,
            feishu_file_key: None,
            feishu_resource_type: None,
            wecom_download_url: None,
            wecom_aes_key: None,
            dingtalk_download_code: None,
            weixin_encrypt_query_param: Some(encrypt_query_param),
            weixin_aes_key: aes_key,
            weixin_image_aeskey_hex: image_aeskey_hex,
            weixin_voice_encode_type: None,
            weixin_voice_sample_rate: None,
            weixin_voice_asr_text: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundMessage {
    pub channel: String,
    pub account_id: String,
    pub message_id: String,
    pub conversation_key: String,
    pub sender_id: String,
    pub sender_name: Option<String>,
    pub text: String,
    pub is_group: bool,
    pub mentioned_bot: bool,
    #[serde(default)]
    pub reply_context: Option<InboundReplyContext>,
    #[serde(default)]
    pub attachments: Vec<InboundMediaRef>,
}

impl InboundMessage {
    /// Dedup key including attachment identity when the same message_id carries distinct media.
    pub fn dedup_key(&self) -> String {
        if self.attachments.is_empty() {
            return self.message_id.clone();
        }
        let mut parts: Vec<String> = self
            .attachments
            .iter()
            .filter_map(|a| {
                a.feishu_image_key
                    .clone()
                    .or_else(|| a.feishu_file_key.clone())
                    .or_else(|| a.wecom_download_url.clone())
                    .or_else(|| a.dingtalk_download_code.clone())
                    .or_else(|| a.weixin_encrypt_query_param.clone())
            })
            .collect();
        parts.sort();
        if parts.is_empty() {
            return self.message_id.clone();
        }
        format!("{}:{}", self.message_id, parts.join(","))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundReplyContext {
    pub session_webhook: Option<String>,
    pub chat_id: Option<String>,
    pub open_id: Option<String>,
    pub context_token: Option<String>,
    /// WeCom WebSocket passive reply req_id (from callback frame headers).
    #[serde(default, rename = "wecomReqId")]
    pub wecom_req_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WebhookResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

pub struct WebhookContext<'a> {
    pub channel: ChannelId,
    pub account_id: &'a str,
    pub account: &'a crate::config::ChannelAccountConfig,
    pub headers: &'a axum::http::HeaderMap,
    pub raw_body: &'a [u8],
    pub method: &'a str,
    pub query: &'a str,
}

#[derive(Debug, Clone)]
pub struct OutboundContext {
    pub channel: String,
    pub account_id: String,
    pub conversation_key: String,
    pub recipient_id: String,
    pub reply_context: Option<InboundReplyContext>,
}

/// Local file bytes to send back to an IM user.
#[derive(Debug, Clone)]
pub struct OutboundMedia {
    pub file_name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
    /// Resolved absolute path (logging / channel-specific metadata).
    pub local_path: Option<std::path::PathBuf>,
}

impl OutboundMedia {
    pub fn is_image(&self) -> bool {
        self.mime_type.starts_with("image/")
    }

    pub fn is_video(&self) -> bool {
        self.mime_type.starts_with("video/")
    }
}

#[async_trait::async_trait]
pub trait ChannelWebhookAdapter: Send + Sync {
    fn channel_id(&self) -> ChannelId;

    async fn handle_webhook(&self, ctx: WebhookContext<'_>) -> anyhow::Result<WebhookResponse>;

    fn parse_inbound(&self, event: &serde_json::Value, account_id: &str) -> Option<InboundMessage>;
}

#[async_trait::async_trait]
pub trait ChannelOutboundAdapter: Send + Sync {
    fn channel_id(&self) -> ChannelId;

    async fn send_text(&self, ctx: OutboundContext, text: &str) -> anyhow::Result<()>;

    async fn send_media(
        &self,
        ctx: OutboundContext,
        caption: Option<&str>,
        media: OutboundMedia,
    ) -> anyhow::Result<()> {
        let _ = (ctx, caption, media);
        Err(anyhow::anyhow!(
            "channel {} does not support outbound media",
            self.channel_id()
        ))
    }
}

pub struct ChannelPlugin {
    pub webhook: std::sync::Arc<dyn ChannelWebhookAdapter>,
    pub outbound: std::sync::Arc<dyn ChannelOutboundAdapter>,
}

impl ChannelPlugin {
    pub fn new(
        webhook: std::sync::Arc<dyn ChannelWebhookAdapter>,
        outbound: std::sync::Arc<dyn ChannelOutboundAdapter>,
    ) -> Self {
        Self { webhook, outbound }
    }

    pub fn channel_id(&self) -> ChannelId {
        self.webhook.channel_id()
    }
}
