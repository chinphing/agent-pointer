pub mod ilink_client;
pub mod monitor;
pub mod qr_login;
mod outbound;
mod webhook;

use std::sync::Arc;

use crate::traits::ChannelPlugin;

pub fn build_plugin() -> ChannelPlugin {
    ChannelPlugin::new(
        Arc::new(webhook::WeixinWebhook),
        Arc::new(outbound::WeixinOutbound::default()),
    )
}
