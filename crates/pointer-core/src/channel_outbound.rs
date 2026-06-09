//! IM channel outbound bridge (installed by `pointer-channels` at startup).

use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

const IM_CHANNELS: &[&str] = &["feishu", "dingtalk", "wecom", "weixin"];

#[derive(Debug, Clone)]
pub struct ChannelOutboundRequest {
    pub conversation_id: String,
    pub text: Option<String>,
    pub media_paths: Vec<String>,
}

pub type ChannelOutboundSender =
    Arc<dyn Fn(ChannelOutboundRequest) -> Result<()> + Send + Sync>;

static SENDER: OnceLock<ChannelOutboundSender> = OnceLock::new();
static ACTIVE_SESSIONS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashSet<String>> {
    ACTIVE_SESSIONS.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn is_im_conversation(conversation_id: &str) -> bool {
    let channel = conversation_id.split(':').next().unwrap_or("");
    IM_CHANNELS.contains(&channel)
}

pub fn set_sender(sender: ChannelOutboundSender) {
    let _ = SENDER.set(sender);
}

pub fn sender_configured() -> bool {
    SENDER.get().is_some()
}

pub fn register_im_session(conversation_id: &str) {
    if is_im_conversation(conversation_id) {
        sessions().lock().insert(conversation_id.to_string());
    }
}

pub fn unregister_im_session(conversation_id: &str) {
    sessions().lock().remove(conversation_id);
}

pub fn is_active_im_session(conversation_id: &str) -> bool {
    sessions().lock().contains(conversation_id)
}

pub fn send_channel_outbound(req: ChannelOutboundRequest) -> Result<()> {
    if !is_active_im_session(&req.conversation_id) {
        return Err(anyhow!(
            "channel_message: not an active IM session ({})",
            req.conversation_id
        ));
    }
    let sender = SENDER
        .get()
        .ok_or_else(|| anyhow!("channel outbound bridge not configured"))?;
    sender(req)
}
