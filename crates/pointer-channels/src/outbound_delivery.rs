use anyhow::Result;

use crate::gateway::ChannelGateway;
use crate::outbound_resolve::{
    format_im_download_link_message, resolve_im_outbound_media, resolve_outbound_media,
    ImOutboundMediaDelivery,
};

pub async fn deliver_outbound_explicit(
    gateway: &ChannelGateway,
    ctx: crate::traits::OutboundContext,
    text: Option<&str>,
    media_paths: &[String],
) -> Result<()> {
    let plugin = gateway
        .registry()
        .get(&ctx.channel)
        .ok_or_else(|| anyhow::anyhow!("unknown channel {}", ctx.channel))?;
    if let Some(t) = text.filter(|s| !s.trim().is_empty()) {
        plugin.outbound.send_text(ctx.clone(), t).await?;
    }
    for raw in media_paths {
        match resolve_im_outbound_media(raw) {
            Ok(ImOutboundMediaDelivery::Direct(media)) => {
                plugin
                    .outbound
                    .send_media(ctx.clone(), None, media)
                    .await?;
            }
            Ok(ImOutboundMediaDelivery::DownloadLink {
                url,
                file_name,
                size_bytes,
            }) => {
                let line = format_im_download_link_message(&file_name, size_bytes, &url);
                plugin.outbound.send_text(ctx.clone(), &line).await?;
            }
            Err(e) => {
                // Fall back to legacy resolve for non-IM size paths (e.g. tests).
                log::warn!("im outbound resolve failed for {raw}: {e:#}; trying direct resolve");
                let resolved = resolve_outbound_media(raw)?;
                plugin
                    .outbound
                    .send_media(ctx.clone(), None, resolved.media)
                    .await?;
            }
        }
    }
    Ok(())
}
