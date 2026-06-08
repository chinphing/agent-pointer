use std::collections::HashMap;
use std::sync::Arc;

use crate::traits::ChannelPlugin;

pub struct ChannelRegistry {
    plugins: HashMap<&'static str, Arc<ChannelPlugin>>,
}

impl ChannelRegistry {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    pub fn register(&mut self, plugin: Arc<ChannelPlugin>) {
        self.plugins.insert(plugin.channel_id(), plugin);
    }

    pub fn get(&self, channel: &str) -> Option<Arc<ChannelPlugin>> {
        self.plugins.get(channel).cloned()
    }

    pub fn list_ids(&self) -> Vec<&'static str> {
        self.plugins.keys().copied().collect()
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}
