pub mod media;
pub mod monitor;
mod outbound;
pub mod parse;
mod webhook;
mod ws_client;
mod ws_state;

use std::sync::Arc;

use crate::traits::ChannelPlugin;

pub fn build_plugin() -> ChannelPlugin {
    ChannelPlugin::new(
        Arc::new(webhook::WeComWebhook),
        Arc::new(outbound::WeComOutbound::default()),
    )
}
