use anyhow::Result;

use crate::gateway::ChannelGateway;
use crate::outbound_resolve::resolve_outbound_media_with_policy;

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
    let media_roots = gateway.config().meta.media_local_roots.clone();

    if let Some(t) = text.filter(|s| !s.trim().is_empty()) {
        plugin.outbound.send_text(ctx.clone(), t).await?;
    }
    for raw in media_paths {
        let resolved = resolve_outbound_media_with_policy(raw, &media_roots)?;
        plugin
            .outbound
            .send_media(ctx.clone(), None, resolved.media)
            .await?;
    }
    Ok(())
}
