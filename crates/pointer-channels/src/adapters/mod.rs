pub mod dingtalk;
pub mod feishu;
pub mod wecom;
pub mod weixin;

use std::sync::Arc;

use crate::registry::ChannelRegistry;
pub fn register_builtin_channels(registry: &mut ChannelRegistry) {
    registry.register(Arc::new(feishu::build_plugin()));
    registry.register(Arc::new(dingtalk::build_plugin()));
    registry.register(Arc::new(wecom::build_plugin()));
    registry.register(Arc::new(weixin::build_plugin()));
}
