use std::sync::Arc;
use std::sync::OnceLock;

use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::gateway::ChannelGateway;

static STANDALONE_MONITOR_RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

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

    fn spawn_handle() -> tokio::runtime::Handle {
        tokio::runtime::Handle::try_current().unwrap_or_else(|_| {
            STANDALONE_MONITOR_RT
                .get_or_init(|| {
                    tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .thread_name("channel-monitors")
                        .build()
                        .expect("channel monitors tokio runtime")
                })
                .handle()
                .clone()
        })
    }

    fn spawn_monitors(gateway: Arc<ChannelGateway>, cancel: CancellationToken, reason: &'static str) {
        Self::spawn_handle().spawn(async move {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_does_not_panic_without_tokio_runtime() {
        // Tauri `.setup()` and sync IPC commands have no Tokio context.
        let supervisor = MonitorSupervisor::new();
        let core = pointer_core::chat_service::AppState::new();
        let registry = crate::registry::ChannelRegistry::new();
        let gateway = Arc::new(
            ChannelGateway::new(Arc::new(core), registry).expect("gateway"),
        );
        supervisor.start(gateway);
    }
}
