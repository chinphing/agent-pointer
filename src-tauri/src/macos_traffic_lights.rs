//! Reposition macOS traffic lights (Tauri config alone may not apply after runtime chrome changes).

use tauri::LogicalPosition;

/// Horizontal offset from window leading edge; vertical inset into title bar (larger = lower).
pub const INSET_X: f64 = 12.0;
pub const INSET_Y: f64 = 17.0;

/// Apply traffic-light inset on the main thread after the window exists.
pub fn apply_inset(ns_window: *mut std::ffi::c_void, position: LogicalPosition<f64>) {
    // SAFETY: pointer from `WebviewWindow::ns_window()` on the main thread.
    unsafe { inset_traffic_lights(&*(ns_window.cast()), position) }
}

/// Logic aligned with Tao `inset_traffic_lights` (tao 0.35 / overlay title bar).
unsafe fn inset_traffic_lights(window: &objc2_app_kit::NSWindow, position: LogicalPosition<f64>) {
    use objc2::msg_send;
    use objc2_app_kit::{NSView, NSWindowButton};
    use objc2_foundation::NSRect;

    let (x, y) = (position.x, position.y);

    let Some(close) = window.standardWindowButton(NSWindowButton::CloseButton) else {
        log::warn!("macOS traffic lights: close button not found");
        return;
    };
    let Some(miniaturize) = window.standardWindowButton(NSWindowButton::MiniaturizeButton) else {
        log::warn!("macOS traffic lights: minimize button not found");
        return;
    };
    let Some(zoom) = window.standardWindowButton(NSWindowButton::ZoomButton) else {
        log::warn!("macOS traffic lights: zoom button not found");
        return;
    };

    let Some(title_bar_container_view) = close.superview().and_then(|v| v.superview()) else {
        log::warn!("macOS traffic lights: title bar container not found");
        return;
    };

    let close_rect = NSView::frame(close.as_ref());
    let title_bar_frame_height = close_rect.size.height + y;
    let mut title_bar_rect = NSView::frame(title_bar_container_view.as_ref());
    title_bar_rect.size.height = title_bar_frame_height;
    title_bar_rect.origin.y = window.frame().size.height - title_bar_frame_height;
    let title_bar_view: &NSView = title_bar_container_view.as_ref();
    let _: () = msg_send![title_bar_view, setFrame: title_bar_rect];

    let space_between =
        NSView::frame(miniaturize.as_ref()).origin.x - close_rect.origin.x;
    let window_buttons = [close, miniaturize, zoom];

    for (i, button) in window_buttons.into_iter().enumerate() {
        let mut rect = NSView::frame(button.as_ref());
        rect.origin.x = x + (i as f64 * space_between);
        button.setFrameOrigin(rect.origin);
    }
}
