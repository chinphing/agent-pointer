//! IM channel gateway: Webhook-first adapters for Feishu, DingTalk, WeCom, and Weixin (iLink).

pub mod adapters;
pub mod api;
pub mod config;
pub mod credentials;
pub mod crypto;
pub mod dedup;
pub mod dispatch;
pub mod gateway;
pub mod http_client;
pub mod inbound;
pub mod pairing;
pub mod registry;
pub mod session;
pub mod traits;
pub mod webhook;

pub use config::{load_channels_config, save_channels_config, ChannelsConfig};
pub use gateway::ChannelGateway;
pub use registry::ChannelRegistry;
pub use traits::{ChannelId, ChannelPlugin, InboundMessage, WebhookResponse};
