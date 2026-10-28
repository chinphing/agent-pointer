//! Computer-only agent extension hooks (screen inject, etc.).
//!
//! Generic extension **points** and [`crate::extensions::ExtensionRegistry`] live in `crate::extensions`;
//! this module registers hooks that only apply to the Computer agent profile.

/// `[CUR_SCREEN]` inject hook. Public so `tests/computer_e2e` can reuse the
/// exact preamble and slot labels production builds, instead of a copy that
/// would drift the moment the prompt changes.
pub mod screen_inject;
mod tier_dynamic;

use crate::extensions::ExtensionRegistry;

/// Register all Computer agent hooks with the global registry.
pub fn register(registry: &mut ExtensionRegistry) {
    screen_inject::register(registry);
    tier_dynamic::register(registry);
}
