use std::sync::Arc;

use pointer_core::channel_outbound::{self, ChannelOutboundRequest};
use pointer_core::tools::ToolRegistry;

use crate::gateway::ChannelGateway;
use crate::outbound_delivery;

pub fn install_channel_outbound_bridge(gateway: Arc<ChannelGateway>, tools: Arc<ToolRegistry>) {
    pointer_core::tools::channel_message::register(&tools);

    let gw = gateway.clone();
    channel_outbound::set_sender(Arc::new(move |req: ChannelOutboundRequest| {
        let gw = gw.clone();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                outbound_delivery::deliver_outbound(
                    &gw,
                    &req.conversation_id,
                    req.text.as_deref(),
                    &req.media_paths,
                )
                .await
            })
        })
    }));
    log::info!("channel outbound bridge installed (channel_message tool + MEDIA: delivery)");
}
