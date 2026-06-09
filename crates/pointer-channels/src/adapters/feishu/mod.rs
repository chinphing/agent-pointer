pub mod auth;
pub mod media;
mod monitor;
mod outbound;
mod parse;
mod webhook;
mod ws_client;

pub use monitor::run_feishu_monitor;
pub use parse::parse_feishu_event;

use std::sync::Arc;

use crate::traits::ChannelPlugin;

pub fn build_plugin() -> ChannelPlugin {
    ChannelPlugin::new(
        Arc::new(webhook::FeishuWebhook),
        Arc::new(outbound::FeishuOutbound::default()),
    )
}
