//! QR-based channel credential registration (Feishu / DingTalk / WeCom device flows).

mod dingtalk;
mod feishu;
mod persist;
mod qr;
mod state;
mod wecom;

pub use persist::apply_registration_session_to_config;
pub use state::{ChannelRegistrationState, RegistrationCompletion, RegistrationSession};
