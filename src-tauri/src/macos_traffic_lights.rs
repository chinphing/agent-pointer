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

    let (x, y) = (position.x, position.y);

    let Some(close) = window.standardWindowButton(NSWindowButton::CloseButton) else {
        log::debug!("macOS traffic lights: close button not found");
        return;
    };
    let Some(miniaturize) = window.standardWindowButton(NSWindowButton::MiniaturizeButton) else {
        log::debug!("macOS traffic lights: minimize button not found");
        return;
    };
    let Some(zoom) = window.standardWindowButton(NSWindowButton::ZoomButton) else {
        log::debug!("macOS traffic lights: zoom button not found");
        return;
    };

    let Some(title_bar_container_view) = close.superview().and_then(|v| v.superview()) else {
        log::debug!("macOS traffic lights: title bar container not found");
        return;
    };

    let close_rect = NSView::frame(close.as_ref());
    let title_bar_frame_height = close_rect.size.height + y;
    let mut title_bar_rect = NSView::frame(title_bar_container_view.as_ref());
    title_bar_rect.size.height = title_bar_frame_height;
    title_bar_rect.origin.y = window.frame().size.height - title_bar_frame_height;
    let title_bar_view: &NSView = title_bar_container_view.as_ref();
    let _: () = msg_send![title_bar_view, setFrame: title_bar_rect];

    let space_between = NSView::frame(miniaturize.as_ref()).origin.x - close_rect.origin.x;
    let window_buttons = [close, miniaturize, zoom];

    for (i, button) in window_buttons.into_iter().enumerate() {
        let mut rect = NSView::frame(button.as_ref());
        rect.origin.x = x + (i as f64 * space_between);
        button.setFrameOrigin(rect.origin);
    }
}

/// Hide or show standard close / minimize / zoom buttons (computer compact dock bar).
pub fn set_traffic_lights_visible(ns_window: *mut std::ffi::c_void, visible: bool) {
    // SAFETY: pointer from `WebviewWindow::ns_window()` on the main thread.
    unsafe { set_traffic_lights_visible_inner(&*(ns_window.cast()), visible) }
}

unsafe fn set_traffic_lights_visible_inner(window: &objc2_app_kit::NSWindow, visible: bool) {
    use objc2_app_kit::NSView;
    use objc2_app_kit::NSWindowButton;

    let hidden = !visible;
    for kind in [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ] {
        if let Some(button) = window.standardWindowButton(kind) {
            NSView::setHidden(button.as_ref(), hidden);
        }
    }
}

/// Overlay title bar (traffic lights only, no visible title strip) after runtime decoration changes.
pub fn apply_overlay_titlebar(ns_window: *mut std::ffi::c_void) {
    // SAFETY: pointer from `WebviewWindow::ns_window()` on the main thread.
    unsafe {
        use objc2_app_kit::{NSWindow, NSWindowTitleVisibility};
        let window = &*(ns_window.cast::<NSWindow>());
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setTitlebarAppearsTransparent(true);
    }
}

const COMPACT_CORNER_RADIUS: f64 = 14.0;

unsafe fn apply_content_corner_radius(window: &objc2_app_kit::NSWindow, radius: f64) {
    use objc2::msg_send;
    use objc2_app_kit::NSView;

    let Some(content) = window.contentView() else {
        return;
    };
    let cv: &NSView = content.as_ref();
    NSView::setWantsLayer(cv, true);
    let layer: *mut objc2::runtime::AnyObject = msg_send![cv, layer];
    if layer.is_null() {
        return;
    }
    let _: () = msg_send![layer, setCornerRadius: radius];
    let _: () = msg_send![layer, setMasksToBounds: radius > 0.0];
}

/// Transparent rounded window surface for compact dock bar; reset when restoring full UI.
pub fn set_compact_surface(ns_window: *mut std::ffi::c_void, compact: bool) {
    // SAFETY: pointer from `WebviewWindow::ns_window()` on the main thread.
    unsafe {
        use objc2_app_kit::{NSColor, NSWindow};
        let window = &*(ns_window.cast::<NSWindow>());
        if compact {
            window.setOpaque(false);
            window.setBackgroundColor(Some(&NSColor::clearColor()));
            window.setHasShadow(true);
            apply_content_corner_radius(window, COMPACT_CORNER_RADIUS);
            disable_window_background_drag(ns_window);
        } else {
            apply_content_corner_radius(window, 0.0);
            window.setOpaque(true);
            window.setBackgroundColor(None);
            window.setHasShadow(true);
            enable_window_dragging(ns_window);
        }
    }
}

/// Apply saved inner size + outer top-left on the main thread (Tauri `set_size` is async on macOS).
pub fn set_window_geometry(
    ns_window: *mut std::ffi::c_void,
    logical_inner_w: f64,
    logical_inner_h: f64,
    logical_outer_x: f64,
    logical_outer_y: f64,
) {
    // SAFETY: pointer from `WebviewWindow::ns_window()` on the main thread.
    unsafe {
        use objc2::MainThreadMarker;
        use objc2_app_kit::{NSScreen, NSWindow};
        use objc2_foundation::{NSPoint, NSSize};

        let window = &*(ns_window.cast::<NSWindow>());
        window.setContentSize(NSSize::new(logical_inner_w, logical_inner_h));

        let screen_h = MainThreadMarker::new()
            .and_then(|mtm| NSScreen::mainScreen(mtm))
            .map(|s| s.frame().size.height)
            .unwrap_or(0.0);
        let point = NSPoint::new(logical_outer_x, screen_h - logical_outer_y);
        window.setFrameTopLeftPoint(point);
    }
}

/// Disable webview background drag; chrome strips use `startDragging()` only.
pub fn enable_window_dragging(ns_window: *mut std::ffi::c_void) {
    set_window_background_movable(ns_window, false);
}

/// Frameless compact bar: same as full UI — no background drag.
pub fn disable_window_background_drag(ns_window: *mut std::ffi::c_void) {
    set_window_background_movable(ns_window, false);
}

fn set_window_background_movable(ns_window: *mut std::ffi::c_void, movable: bool) {
    // SAFETY: pointer from `WebviewWindow::ns_window()` on the main thread.
    unsafe {
        use objc2::msg_send;
        use objc2_app_kit::NSWindow;
        let window = &*(ns_window.cast::<NSWindow>());
        let _: () = msg_send![window, setMovableByWindowBackground: movable];
    }
}
