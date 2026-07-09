use std::sync::Arc;

use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::gateway::ChannelGateway;

/// Cancels and re-spawns IM channel background monitors after config changes.
pub struct MonitorSupervisor {
    cancel: Mutex<CancellationToken>,
}

impl MonitorSupervisor {
    pub fn new() -> Self {
        Self {
            cancel: Mutex::new(CancellationToken::new()),
        }
    }

    pub fn start(&self, gateway: Arc<ChannelGateway>) {
        let cancel = self.cancel.lock().clone();
        Self::spawn_monitors(gateway, cancel, "initial");
    }

    pub fn restart(&self, gateway: Arc<ChannelGateway>) {
        let cancel = {
            let mut guard = self.cancel.lock();
            guard.cancel();
            *guard = CancellationToken::new();
            guard.clone()
        };
        Self::spawn_monitors(gateway, cancel, "restart");
    }

    fn spawn_monitors(gateway: Arc<ChannelGateway>, cancel: CancellationToken, reason: &'static str) {
        tokio::spawn(async move {
            log::info!("channel monitors: spawning im channel background tasks ({reason})");
            gateway.spawn_wecom_monitors(cancel.clone());
            gateway.spawn_feishu_monitors(cancel.clone());
            gateway.spawn_dingtalk_monitors(cancel.clone());
            gateway.spawn_weixin_monitors(cancel);
        });
    }
}

impl Default for MonitorSupervisor {
    fn default() -> Self {
        Self::new()
    }
}
