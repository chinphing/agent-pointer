//! IM channel startup hook.
//!
//! Wires the Run → IM outbound bus:
//! - registers the `im_send` tool so agents can push to IM dynamically during
//!   a run (the **dynamic** path);
//! - the **static** path (cron `deliver` field → `ImDeliverHook`) is registered
//!   by the host into the dispatcher at construction time (see
//!   `AppState::build_dispatcher_with_extra_finished_hooks`), not here, because
//!   it needs the dispatcher's hook registry rather than the tool registry.

use std::sync::Arc;

use pointer_core::tools::ToolRegistry;

use crate::gateway::ChannelGateway;
use crate::im_delivery;
use crate::im_send;

pub fn install_channel_outbound_bridge(gateway: Arc<ChannelGateway>, tools: Arc<ToolRegistry>) {
    im_send::register(&tools, gateway.clone());
    let gw = gateway.clone();
    pointer_core::tools::cron_job::set_deliver_validator(Some(Arc::new(move |deliver| {
        let cfg = gw.config().clone();
        im_delivery::validate_deliver_spec(deliver, &cfg)
    })));
    log::info!(
        "IM outbound: im_send tool registered; cron_job deliver validator installed; static delivery via ImDeliverHook (host-registered)"
    );
}
