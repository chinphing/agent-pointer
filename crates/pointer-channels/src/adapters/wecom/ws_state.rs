use std::collections::HashMap;
use std::sync::OnceLock;

use parking_lot::Mutex;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum WsOutboundCmd {
    StreamReply {
        req_id: String,
        stream_id: String,
        content: String,
        finish: bool,
    },
    SendMarkdown {
        chat_id: String,
        content: String,
    },
}

#[derive(Clone)]
pub struct WeComWsSession {
    pub account_id: String,
    outbound_tx: mpsc::UnboundedSender<WsOutboundCmd>,
}

impl WeComWsSession {
    pub fn new(account_id: String, outbound_tx: mpsc::UnboundedSender<WsOutboundCmd>) -> Self {
        Self {
            account_id,
            outbound_tx,
        }
    }

    pub fn send_stream_reply(
        &self,
        req_id: &str,
        stream_id: &str,
        content: &str,
        finish: bool,
    ) -> anyhow::Result<()> {
        self.outbound_tx
            .send(WsOutboundCmd::StreamReply {
                req_id: req_id.to_string(),
                stream_id: stream_id.to_string(),
                content: content.to_string(),
                finish,
            })
            .map_err(|e| anyhow::anyhow!("wecom ws outbound channel closed: {e}"))
    }

    pub fn send_markdown(&self, chat_id: &str, content: &str) -> anyhow::Result<()> {
        self.outbound_tx
            .send(WsOutboundCmd::SendMarkdown {
                chat_id: chat_id.to_string(),
                content: content.to_string(),
            })
            .map_err(|e| anyhow::anyhow!("wecom ws outbound channel closed: {e}"))
    }
}

static SESSIONS: OnceLock<Mutex<HashMap<String, WeComWsSession>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<String, WeComWsSession>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn register_session(session: WeComWsSession) {
    sessions()
        .lock()
        .insert(session.account_id.clone(), session);
}

pub fn unregister_session(account_id: &str) {
    sessions().lock().remove(account_id);
}

pub fn get_session(account_id: &str) -> Option<WeComWsSession> {
    sessions().lock().get(account_id).cloned()
}

pub fn is_connected(account_id: &str) -> bool {
    sessions().lock().contains_key(account_id)
}
