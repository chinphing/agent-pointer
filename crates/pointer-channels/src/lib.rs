//! IM channel gateway: Webhook-first adapters for Feishu, DingTalk, WeCom, and Weixin (iLink).

pub mod adapters;
pub mod bridge;
pub mod api;
pub mod config;
pub mod connection_state;
pub mod credentials;
pub mod crypto;
pub mod dedup;
pub mod dispatch;
pub mod gateway;
pub mod http_client;
pub mod inbound;
pub mod media;
mod media_roots;
mod outbound_delivery;
mod outbound_format;
pub mod outbound_reply;
pub mod outbound_resolve;
pub mod pairing;
pub mod registration;
pub mod registry;
pub mod session;
pub mod session_fork;
pub mod session_abort;
pub mod session_agent;
pub mod session_reset;
pub mod traits;
pub mod webhook;

pub use config::{load_channels_config, save_channels_config, ChannelsConfig};
pub use connection_state::{is_connected as is_channel_runtime_connected, set_connected};
pub use bridge::install_channel_outbound_bridge;
pub use gateway::ChannelGateway;
pub use registration::{ChannelRegistrationState, RegistrationSession};
pub use registry::ChannelRegistry;
pub use traits::{ChannelId, ChannelPlugin, InboundMessage, WebhookResponse};
