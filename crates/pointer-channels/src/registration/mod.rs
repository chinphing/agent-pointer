//! QR-based channel credential registration (Feishu / DingTalk device flows).

mod dingtalk;
mod feishu;
mod qr;
mod state;

pub use state::{ChannelRegistrationState, RegistrationSession};
