pub mod auth;
pub mod media;
pub mod monitor;
mod outbound;
pub mod parse;
mod webhook;
mod ws_client;
mod ws_pending;
mod ws_state;
mod ws_upload;

use std::sync::Arc;

use crate::traits::ChannelPlugin;

pub fn build_plugin() -> ChannelPlugin {
    ChannelPlugin::new(
        Arc::new(webhook::WeComWebhook),
        Arc::new(outbound::WeComOutbound::default()),
    )
}
