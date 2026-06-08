mod outbound;
mod webhook;

use std::sync::Arc;

use crate::traits::ChannelPlugin;

pub fn build_plugin() -> ChannelPlugin {
    ChannelPlugin::new(
        Arc::new(webhook::FeishuWebhook),
        Arc::new(outbound::FeishuOutbound::default()),
    )
}
