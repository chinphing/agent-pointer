use pointer_channels::ChannelGateway;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

pub struct ChannelMonitorHandle {
    pub gateway: Arc<ChannelGateway>,
    cancel: Mutex<CancellationToken>,
}

impl ChannelMonitorHandle {
    pub fn new(gateway: Arc<ChannelGateway>) -> Self {
        Self {
            gateway,
            cancel: Mutex::new(CancellationToken::new()),
        }
    }

    pub fn start(&self) {
        let cancel = self.cancel.lock().expect("channel monitor cancel lock").clone();
        let gateway = self.gateway.clone();
        tauri::async_runtime::spawn(async move {
            log::info!("channel monitors: spawning im channel background tasks");
            gateway.spawn_wecom_monitors(cancel.clone());
            gateway.spawn_feishu_monitors(cancel.clone());
            gateway.spawn_dingtalk_monitors(cancel.clone());
            gateway.spawn_weixin_monitors(cancel);
        });
    }

    pub fn restart(&self) {
        let new_cancel = {
            let mut guard = self.cancel.lock().expect("channel monitor cancel lock");
            guard.cancel();
            *guard = CancellationToken::new();
            guard.clone()
        };
        let gateway = self.gateway.clone();
        tauri::async_runtime::spawn(async move {
            log::info!("channel monitors: restarted after config update");
            gateway.spawn_wecom_monitors(new_cancel.clone());
            gateway.spawn_feishu_monitors(new_cancel.clone());
            gateway.spawn_dingtalk_monitors(new_cancel.clone());
            gateway.spawn_weixin_monitors(new_cancel);
        });
    }
}
