mod auth;
pub mod media;
mod monitor;
mod outbound;
pub mod parse;
mod stream_client;
mod webhook;

pub use monitor::run_dingtalk_monitor;

use std::sync::Arc;

use crate::traits::ChannelPlugin;

pub fn build_plugin() -> ChannelPlugin {
    ChannelPlugin::new(
        Arc::new(webhook::DingTalkWebhook),
        Arc::new(outbound::DingTalkOutbound::default()),
    )
}
