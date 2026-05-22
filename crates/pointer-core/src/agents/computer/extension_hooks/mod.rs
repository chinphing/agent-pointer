//! Computer-only agent extension hooks (screen inject, etc.).
//!
//! Generic extension **points** and [`crate::extensions::ExtensionRegistry`] live in `crate::extensions`;
//! this module registers hooks that only apply to the Computer agent profile.

mod screen_inject;
mod tier_dynamic;

use crate::extensions::ExtensionRegistry;

/// Register all Computer agent hooks with the global registry.
pub fn register(registry: &mut ExtensionRegistry) {
    screen_inject::register(registry);
    tier_dynamic::register(registry);
}
