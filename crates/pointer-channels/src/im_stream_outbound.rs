//! Mirror agent stream events to IM customers during a channel dispatch run.

use std::collections::HashSet;

use anyhow::Result;
use pointer_core::models::StreamEvent;

use crate::config::ImOutboundConfig;
use crate::outbound_reply::{im_outbound_reply_source, split_reply_media};
use crate::outbound_resolve::{
    format_im_download_link_message, resolve_im_outbound_media,
    try_im_download_link_for_path, ImOutboundMediaDelivery,
};
use crate::traits::{ChannelPlugin, OutboundContext};

pub struct ImStreamOutbound<'a> {
    plugin: &'a ChannelPlugin,
    outbound: OutboundContext,
    cfg: ImOutboundConfig,
    conv_id: String,
    sent_message_ids: HashSet<String>,
    sent_media: HashSet<String>,
    last_sent_visible: String,
}

impl<'a> ImStreamOutbound<'a> {
    pub fn new(
        plugin: &'a ChannelPlugin,
        outbound: OutboundContext,
        cfg: ImOutboundConfig,
        conv_id: String,
    ) -> Self {
        Self {
            plugin,
            outbound,
            cfg,
            conv_id,
            sent_message_ids: HashSet::new(),
            sent_media: HashSet::new(),
            last_sent_visible: String::new(),
        }
    }

    pub async fn on_event(&mut self, ev: &StreamEvent) -> Result<()> {
        match ev {
            StreamEvent::MessageEnd {
                message_id,
                content,
                raw_content,
                ..
            } => {
                self.handle_message_end(message_id, content.as_deref(), raw_content.as_deref())
                    .await
            }
            // Tool progress stays in the App; IM only gets assistant / clarify text.
            StreamEvent::ToolCallStatus { .. } => Ok(()),
            _ => Ok(()),
        }
    }

    /// Send any reply text/media not already pushed during the stream.
    pub async fn finish(&mut self, reply_text: &str) -> Result<bool> {
        let (visible_text, media_refs) = split_reply_media(reply_text);
        let mut sent_any = false;

        if !visible_text.trim().is_empty() {
            let should_send = if self.cfg.send_intermediate_text {
                visible_text.trim() != self.last_sent_visible.trim()
            } else {
                true
            };
            if should_send {
                self.plugin
                    .outbound
                    .send_text(self.outbound.clone(), &visible_text)
                    .await?;
                self.last_sent_visible = visible_text.clone();
                sent_any = true;
            }
        }

        if self.send_media_refs(&media_refs).await? {
            sent_any = true;
        }

        Ok(sent_any)
    }

    async fn handle_message_end(
        &mut self,
        message_id: &str,
        content: Option<&str>,
        raw_content: Option<&str>,
    ) -> Result<()> {
        // Hermes clarify: always deliver ask_user options even if intermediate text is off.
        let force_ask_user_clarify = message_id.starts_with("im-ask-user-");
        if !self.cfg.send_intermediate_text && !force_ask_user_clarify {
            return Ok(());
        }
        if self.sent_message_ids.contains(message_id) {
            log::info!(
                "im_stream_outbound handle_message_end SKIP dup message_id={}",
                message_id
            );
            return Ok(());
        }
        log::info!(
            "im_stream_outbound handle_message_end message_id={} content_len={} raw_len={}",
            message_id,
            content.map(str::len).unwrap_or(0),
            raw_content.map(str::len).unwrap_or(0),
        );
        let outbound = im_outbound_reply_source(raw_content, content);
        if outbound.trim().is_empty() {
            return Ok(());
        }

        let sent = self.send_visible_chunk(&outbound).await?;
        if sent {
            self.sent_message_ids.insert(message_id.to_string());
        }
        Ok(())
    }

    async fn send_visible_chunk(&mut self, raw: &str) -> Result<bool> {
        let (visible, media_refs) = split_reply_media(raw);
        let mut sent_any = false;
        if !visible.trim().is_empty() {
            self.plugin
                .outbound
                .send_text(self.outbound.clone(), &visible)
                .await?;
            self.last_sent_visible = visible.clone();
            sent_any = true;
        }
        if self.send_media_refs(&media_refs).await? {
            sent_any = true;
        }
        Ok(sent_any)
    }

    async fn send_media_refs(&mut self, media_refs: &[String]) -> Result<bool> {
        let mut sent_any = false;
        for raw_path in media_refs {
            if !self.sent_media.insert(raw_path.clone()) {
                continue;
            }
            match resolve_im_outbound_media(raw_path) {
                Ok(ImOutboundMediaDelivery::Direct(media)) => {
                    if let Err(e) = self
                        .plugin
                        .outbound
                        .send_media(self.outbound.clone(), None, media)
                        .await
                    {
                        log::error!(
                            "channel outbound media failed conv={} path={raw_path}: {e:#}",
                            self.conv_id
                        );
                        if let Ok(ImOutboundMediaDelivery::DownloadLink {
                            url,
                            file_name,
                            size_bytes,
                        }) = try_im_download_link_for_path(raw_path)
                        {
                            let line =
                                format_im_download_link_message(&file_name, size_bytes, &url);
                            match self
                                .plugin
                                .outbound
                                .send_text(self.outbound.clone(), &line)
                                .await
                            {
                                Ok(()) => {
                                    log::info!(
                                        "channel outbound media fell back to download link conv={} path={raw_path}",
                                        self.conv_id
                                    );
                                    sent_any = true;
                                }
                                Err(link_err) => {
                                    log::error!(
                                        "channel outbound download-link fallback failed conv={} path={raw_path}: {link_err:#}",
                                        self.conv_id
                                    );
                                }
                            }
                        }
                    } else {
                        sent_any = true;
                    }
                }
                Ok(ImOutboundMediaDelivery::DownloadLink {
                    url,
                    file_name,
                    size_bytes,
                }) => {
                    let line = format_im_download_link_message(&file_name, size_bytes, &url);
                    if let Err(e) = self
                        .plugin
                        .outbound
                        .send_text(self.outbound.clone(), &line)
                        .await
                    {
                        log::error!(
                            "channel outbound download-link failed conv={} path={raw_path}: {e:#}",
                            self.conv_id
                        );
                    } else {
                        sent_any = true;
                    }
                }
                Err(e) => {
                    log::error!(
                        "channel outbound media resolve failed conv={} path={raw_path}: {e:#}",
                        self.conv_id
                    );
                }
            }
        }
        Ok(sent_any)
    }
}
