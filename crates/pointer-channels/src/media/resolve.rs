use anyhow::Result;
use pointer_core::models::MediaAttachment;

use crate::adapters::dingtalk::media as dingtalk_media;
use crate::adapters::feishu::media as feishu_media;
use crate::adapters::wecom::media as wecom_media;
use crate::adapters::weixin::media as weixin_media;
use crate::config::ChannelAccountConfig;
use crate::http_client::HttpClient;
use crate::media::attachment::to_media_attachment;
use crate::traits::InboundMessage;

pub async fn resolve_feishu_attachments(
    http: &HttpClient,
    account: &ChannelAccountConfig,
    msg: &InboundMessage,
) -> Vec<MediaAttachment> {
    let mut out = Vec::new();
    for media_ref in &msg.attachments {
        match feishu_media::download_inbound_ref(http, account, &msg.message_id, media_ref).await {
            Ok(downloaded) => {
                log::info!(
                    "feishu media downloaded message={} kind={} bytes={}",
                    msg.message_id,
                    media_ref.kind,
                    downloaded.bytes.len()
                );
                out.push(feishu_media::to_media_attachment(downloaded, &media_ref.kind));
            }
            Err(e) => {
                log::warn!(
                    "feishu media download failed message={} kind={}: {e:#}",
                    msg.message_id,
                    media_ref.kind
                );
            }
        }
    }
    out
}

pub async fn resolve_wecom_attachments(
    http: &HttpClient,
    account: &ChannelAccountConfig,
    msg: &InboundMessage,
) -> Vec<MediaAttachment> {
    let mut out = Vec::new();
    for media_ref in &msg.attachments {
        match wecom_media::download_inbound_ref(http, Some(account), media_ref).await {
            Ok(downloaded) => {
                log::info!(
                    "wecom media downloaded message={} kind={} bytes={}",
                    msg.message_id,
                    media_ref.kind,
                    downloaded.bytes.len()
                );
                out.push(to_media_attachment(downloaded, &media_ref.kind));
            }
            Err(e) => {
                log::warn!(
                    "wecom media download failed message={} kind={}: {e:#}",
                    msg.message_id,
                    media_ref.kind
                );
            }
        }
    }
    out
}

pub async fn resolve_dingtalk_attachments(
    http: &HttpClient,
    account: &ChannelAccountConfig,
    msg: &InboundMessage,
) -> Vec<MediaAttachment> {
    let mut out = Vec::new();
    for media_ref in &msg.attachments {
        match dingtalk_media::download_inbound_ref(http, account, media_ref).await {
            Ok(downloaded) => {
                log::info!(
                    "dingtalk media downloaded message={} kind={} bytes={}",
                    msg.message_id,
                    media_ref.kind,
                    downloaded.bytes.len()
                );
                out.push(to_media_attachment(downloaded, &media_ref.kind));
            }
            Err(e) => {
                log::warn!(
                    "dingtalk media download failed message={} kind={}: {e:#}",
                    msg.message_id,
                    media_ref.kind
                );
            }
        }
    }
    out
}

pub async fn resolve_weixin_attachments(
    http: &HttpClient,
    msg: &InboundMessage,
) -> Vec<MediaAttachment> {
    let mut out = Vec::new();
    for media_ref in &msg.attachments {
        match weixin_media::download_inbound_ref(http, media_ref).await {
            Ok(downloaded) => {
                log::info!(
                    "weixin media downloaded message={} kind={} bytes={}",
                    msg.message_id,
                    media_ref.kind,
                    downloaded.bytes.len()
                );
                out.push(weixin_media::to_media_attachment(
                    downloaded,
                    media_ref,
                    &media_ref.kind,
                ));
            }
            Err(e) => {
                log::warn!(
                    "weixin media download failed message={} kind={}: {e:#}",
                    msg.message_id,
                    media_ref.kind
                );
            }
        }
    }
    out
}

pub async fn resolve_inbound_attachments(
    http: &HttpClient,
    account: &ChannelAccountConfig,
    msg: &InboundMessage,
) -> Result<Vec<MediaAttachment>> {
    match msg.channel.as_str() {
        "feishu" => Ok(resolve_feishu_attachments(http, account, msg).await),
        "wecom" => Ok(resolve_wecom_attachments(http, account, msg).await),
        "dingtalk" => Ok(resolve_dingtalk_attachments(http, account, msg).await),
        "weixin" => Ok(resolve_weixin_attachments(http, msg).await),
        _ => Ok(vec![]),
    }
}
