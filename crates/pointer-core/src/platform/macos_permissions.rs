//! macOS TCC checks for Computer agent (effective = API and/or real capture probe).

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
}

/// `CGPreflightScreenCaptureAccess` only — may stay false while Settings already shows enabled.
pub fn screen_recording_preflight() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// True when preflight passes or a real monitor capture succeeds (matches user-visible permission).
pub fn screen_recording_effective() -> bool {
    if screen_recording_preflight() {
        return true;
    }
    screen_recording_probe_capture()
}

pub fn accessibility_effective() -> bool {
    unsafe { AXIsProcessTrusted() }
}

fn screen_recording_probe_capture() -> bool {
    use xcap::Monitor;

    let monitors = match Monitor::all() {
        Ok(m) if !m.is_empty() => m,
        Ok(_) => {
            log::debug!("screen_recording probe: no monitors");
            return false;
        }
        Err(e) => {
            log::debug!("screen_recording probe: Monitor::all failed: {e}");
            return false;
        }
    };
    match monitors[0].capture_image() {
        Ok(img) if img.width() > 0 && img.height() > 0 => {
            log::info!(
                "screen_recording: preflight denied but capture probe ok ({}x{})",
                img.width(),
                img.height()
            );
            true
        }
        Ok(_) => false,
        Err(e) => {
            log::debug!("screen_recording probe: capture_image failed: {e}");
            false
        }
    }
}
