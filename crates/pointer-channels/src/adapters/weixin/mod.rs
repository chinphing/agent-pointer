pub mod cdn;
pub mod cdn_upload;
pub mod context_token;
pub mod ilink_client;
pub mod media;
pub mod monitor;
pub mod parse;
pub mod qr_login;
pub mod silk;
mod outbound;
mod webhook;

pub use monitor::run_weixin_monitor;

use std::sync::Arc;

use crate::traits::ChannelPlugin;

pub fn build_plugin() -> ChannelPlugin {
    ChannelPlugin::new(
        Arc::new(webhook::WeixinWebhook),
        Arc::new(outbound::WeixinOutbound::default()),
    )
}
