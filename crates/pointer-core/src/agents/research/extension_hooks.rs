//! Research agent extension registration (mirrors computer `extension_hooks`).

use crate::extensions::ExtensionRegistry;

/// Register research-specific hooks. Currently a no-op placeholder for symmetry with Computer.
pub fn register(_registry: &mut ExtensionRegistry) {
    log::debug!("research extension_hooks registered");
}
