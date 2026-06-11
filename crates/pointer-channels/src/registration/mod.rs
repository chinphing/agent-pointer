//! QR-based channel credential registration (Feishu / DingTalk / WeCom device flows).

mod dingtalk;
mod feishu;
mod qr;
mod state;
mod wecom;

pub use state::{ChannelRegistrationState, RegistrationSession};
