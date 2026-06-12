//! IM channel startup hook (outbound delivery is via `dispatch` final reply).

use std::sync::Arc;

use pointer_core::tools::ToolRegistry;

use crate::gateway::ChannelGateway;

pub fn install_channel_outbound_bridge(_gateway: Arc<ChannelGateway>, _tools: Arc<ToolRegistry>) {
    log::info!("IM outbound: final assistant reply delivered by dispatch (OpenClaw-aligned)");
}
