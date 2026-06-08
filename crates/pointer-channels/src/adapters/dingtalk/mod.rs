mod monitor;
mod outbound;
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
