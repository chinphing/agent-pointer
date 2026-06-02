//! macOS TCC checks for Computer agent.
//!
//! Screen recording: `CGPreflightScreenCaptureAccess` only.
//! Do **not** use xcap/`capture_image` as a permission signal — without TCC, macOS still
//! returns a non-empty framebuffer (wallpaper / empty desktop, no other app windows).

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

/// `CGPreflightScreenCaptureAccess` — may stay false while Settings already shows enabled.
pub fn screen_recording_preflight() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Register this process in Screen Recording settings (may show dialog if not listed yet).
/// Does not replace preflight; call when opening the permission wizard.
pub fn register_screen_recording_in_settings() -> bool {
    unsafe { CGRequestScreenCaptureAccess() }
}

/// Effective screen-recording permission for the **current process** (same as preflight).
pub fn screen_recording_effective() -> bool {
    screen_recording_preflight()
}

pub fn accessibility_effective() -> bool {
    unsafe { AXIsProcessTrusted() }
}
