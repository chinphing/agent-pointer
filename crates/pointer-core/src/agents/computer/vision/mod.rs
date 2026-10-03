//! Screen capture, annotation, coordinates, overlays, and per-session vision state.

pub mod annotate;
pub mod coord;
pub mod reference_anchors;
#[cfg(feature = "computer")]
pub mod screen;
#[cfg(not(feature = "computer"))]
#[path = "screen_headless.rs"]
pub mod screen;
pub mod screen_overlay;
pub mod vision_state;
#[cfg(windows)]
pub mod windows_gdi;
