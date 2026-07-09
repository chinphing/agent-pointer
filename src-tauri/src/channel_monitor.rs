use pointer_channels::MonitorSupervisor;
use std::sync::Arc;

pub struct ChannelMonitorHandle {
    supervisor: Arc<MonitorSupervisor>,
}

impl ChannelMonitorHandle {
    pub fn new() -> Self {
        Self {
            supervisor: Arc::new(MonitorSupervisor::new()),
        }
    }

    pub fn supervisor(&self) -> Arc<MonitorSupervisor> {
        self.supervisor.clone()
    }

    pub fn start(&self, gateway: Arc<pointer_channels::ChannelGateway>) {
        self.supervisor.start(gateway);
    }
}

impl Default for ChannelMonitorHandle {
    fn default() -> Self {
        Self::new()
    }
}
