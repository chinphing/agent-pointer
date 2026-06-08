mod monitor;
mod outbound;
mod webhook;
mod ws_client;

pub use monitor::run_feishu_monitor;

use std::sync::Arc;

use crate::traits::ChannelPlugin;

pub fn build_plugin() -> ChannelPlugin {
    ChannelPlugin::new(
        Arc::new(webhook::FeishuWebhook),
        Arc::new(outbound::FeishuOutbound::default()),
    )
}
