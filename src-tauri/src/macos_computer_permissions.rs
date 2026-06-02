//! macOS TCC permissions for Computer agent: check and drag-to-list grant guide.

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::PathBuf;

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadOnly, MainThreadMarker, Message};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSDraggingContext, NSDraggingItem, NSDraggingSession, NSDragOperation,
    NSDraggingSource, NSEvent, NSFloatingWindowLevel, NSImage, NSImageAlignment, NSImageScaling,
    NSImageView, NSPanel, NSPasteboardWriting, NSScreen, NSTextAlignment, NSTextField, NSView,
    NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{NSArray, NSBundle, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL};
use serde::Serialize;
use tauri::AppHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionKind {
    ScreenRecording,
    Accessibility,
}

impl PermissionKind {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "screenRecording" | "screen_recording" => Ok(Self::ScreenRecording),
            "accessibility" => Ok(Self::Accessibility),
            other => Err(format!("unknown permission kind: {other}")),
        }
    }

    fn settings_url(self) -> &'static str {
        match self {
            Self::ScreenRecording => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
            Self::Accessibility => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
        }
    }

    fn panel_title(self) -> &'static str {
        match self {
            Self::ScreenRecording => "屏幕录制",
            Self::Accessibility => "辅助功能",
        }
    }

    fn drag_instruction(self) -> &'static str {
        match self {
            Self::ScreenRecording => "拖到右侧「屏幕录制」列表",
            Self::Accessibility => "拖到右侧「辅助功能」列表",
        }
    }

    fn drag_hint(self) -> &'static str {
        let _ = self;
        "拖入后保持启用，将自动进入下一步"
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacosComputerPermissionsStatus {
    /// Effective permission (preflight and/or live capture probe).
    pub screen_recording: bool,
    /// `CGPreflightScreenCaptureAccess` only; may be false while Settings shows enabled.
    pub screen_recording_preflight: bool,
    pub accessibility: bool,
    pub app_bundle_path: String,
    pub executable_path: String,
    pub bundle_id: String,
    pub running_from_app_bundle: bool,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
}

thread_local! {
    static DRAG_PANEL: RefCell<Option<Retained<NSPanel>>> = RefCell::new(None);
}

pub fn status_on_main(app: &AppHandle) -> Result<MacosComputerPermissionsStatus, String> {
    if MainThreadMarker::new().is_some() {
        return Ok(status());
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let app = app.clone();
    app.run_on_main_thread(move || {
        let _ = tx.send(status());
    })
    .map_err(|e| e.to_string())?;
    rx.recv()
        .map_err(|_| "permission status: main thread channel closed".to_string())
}

pub fn status() -> MacosComputerPermissionsStatus {
    let bundle = app_bundle_path();
    let running_from_app_bundle = bundle
        .as_ref()
        .is_some_and(|p| p.extension().and_then(|e| e.to_str()) == Some("app"));
    let preflight = pointer_core::platform::macos_permissions::screen_recording_preflight();
    let screen_effective = pointer_core::platform::macos_permissions::screen_recording_effective();
    let accessibility = pointer_core::platform::macos_permissions::accessibility_effective();
    MacosComputerPermissionsStatus {
        screen_recording: screen_effective,
        screen_recording_preflight: preflight,
        accessibility,
        app_bundle_path: bundle
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        executable_path: std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        bundle_id: bundle_identifier(),
        running_from_app_bundle,
    }
}

fn bundle_identifier() -> String {
    if MainThreadMarker::new().is_some() {
        return NSBundle::mainBundle()
            .bundleIdentifier()
            .map(|s| s.to_string())
            .unwrap_or_default();
    }
    String::new()
}

pub fn open_settings(kind: &str) -> Result<(), String> {
    let kind = PermissionKind::parse(kind)?;
    std::process::Command::new("open")
        .arg(kind.settings_url())
        .spawn()
        .map_err(|e| format!("无法打开系统设置: {e}"))?;
    log::info!("opened macOS settings: {:?}", kind);
    Ok(())
}

pub fn begin_drag_grant_flow(app: &AppHandle, kind: &str) -> Result<(), String> {
    let kind = PermissionKind::parse(kind)?;
    if kind == PermissionKind::Accessibility {
        prompt_accessibility();
    }
    open_settings(match kind {
        PermissionKind::ScreenRecording => "screenRecording",
        PermissionKind::Accessibility => "accessibility",
    })?;
    show_drag_guide(app, kind)
}

pub fn show_drag_guide(app: &AppHandle, kind: PermissionKind) -> Result<(), String> {
    let app = app.clone();
    app.run_on_main_thread(move || {
        if let Err(e) = show_drag_guide_on_main(kind) {
            log::warn!("macOS permission drag guide failed ({kind:?}): {e}");
        }
    })
    .map_err(|e| e.to_string())
}

pub fn dismiss_drag_guide(app: &AppHandle) -> Result<(), String> {
    let app = app.clone();
    app.run_on_main_thread(move || dismiss_drag_panel_on_main())
        .map_err(|e| e.to_string())
}

fn prompt_accessibility() {
    if pointer_core::platform::macos_permissions::accessibility_effective() {
        return;
    }
    unsafe {
        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let dict = CFDictionary::from_CFType_pairs(&[(
            key.as_CFType(),
            CFBoolean::true_value().as_CFType(),
        )]);
        AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef() as *const c_void);
    }
}

fn show_drag_guide_on_main(kind: PermissionKind) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("must run on main thread")?;
    let bundle = app_bundle_path().ok_or("无法定位 Pointer 应用包路径")?;
    let path_ns = NSString::from_str(&bundle.display().to_string());
    let url = NSURL::fileURLWithPath(&path_ns);
    let icon = NSWorkspace::sharedWorkspace().iconForFile(&path_ns);

    dismiss_drag_panel_on_main();

    let panel = build_drag_panel(kind, &url, &icon, mtm)?;
    panel.orderFrontRegardless();
    DRAG_PANEL.with(|cell| {
        *cell.borrow_mut() = Some(panel);
    });
    log::info!(
        "macOS {:?} drag guide shown for {}",
        kind,
        bundle.display()
    );
    Ok(())
}

fn dismiss_drag_panel_on_main() {
    DRAG_PANEL.with(|cell| {
        if let Some(panel) = cell.borrow_mut().take() {
            panel.orderOut(None);
            log::info!("macOS permission drag guide dismissed");
        }
    });
}

fn centered_label(text: &str, frame: NSRect, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFrame(frame);
    label.setAlignment(NSTextAlignment::Center);
    label
}

fn build_drag_panel(
    kind: PermissionKind,
    bundle_url: &NSURL,
    icon: &NSImage,
    mtm: MainThreadMarker,
) -> Result<Retained<NSPanel>, String> {
    let screen = NSScreen::mainScreen(mtm).ok_or("no main screen")?;
    let visible = screen.visibleFrame();
    let width = 272.0;
    let height = 300.0;
    let origin = NSPoint::new(
        visible.origin.x + visible.size.width * 0.2 - width * 0.5,
        visible.origin.y + visible.size.height * 0.5 - height * 0.5,
    );
    let frame = NSRect::new(origin, NSSize::new(width, height));

    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        frame,
        style,
        NSBackingStoreType::Buffered,
        false,
    );
    unsafe {
        panel.setReleasedWhenClosed(false);
    }
    panel.setFloatingPanel(true);
    panel.setLevel(NSFloatingWindowLevel);
    panel.setTitle(&NSString::from_str(kind.panel_title()));
    panel.setHidesOnDeactivate(false);
    panel.setHasShadow(true);
    panel.setOpaque(true);
    let bg = NSColor::controlBackgroundColor();
    panel.setBackgroundColor(Some(&bg));

    let content = panel.contentView().ok_or("panel has no content view")?;
    let pad = 16.0;
    let inner_w = width - pad * 2.0;

    let badge = centered_label(
        kind.panel_title(),
        NSRect::new(
            NSPoint::new(pad, height - pad - 20.0),
            NSSize::new(inner_w, 20.0),
        ),
        mtm,
    );

    let instruction = centered_label(
        kind.drag_instruction(),
        NSRect::new(
            NSPoint::new(pad, height - pad - 48.0),
            NSSize::new(inner_w, 22.0),
        ),
        mtm,
    );

    let icon_side = 96.0;
    let icon_view = DragIconView::new(
        NSRect::new(
            NSPoint::new((width - icon_side) * 0.5, 96.0),
            NSSize::new(icon_side, icon_side),
        ),
        bundle_url,
        icon,
        mtm,
    );

    let hint = centered_label(
        kind.drag_hint(),
        NSRect::new(
            NSPoint::new(pad, pad),
            NSSize::new(inner_w, 40.0),
        ),
        mtm,
    );

    content.addSubview(&badge);
    content.addSubview(&instruction);
    content.addSubview(&icon_view);
    content.addSubview(&hint);

    Ok(panel)
}

pub fn app_bundle_path() -> Option<PathBuf> {
    if MainThreadMarker::new().is_some() {
        let bundle = NSBundle::mainBundle();
        let path = PathBuf::from(bundle.bundlePath().to_string());
        if path.extension().and_then(|e| e.to_str()) == Some("app") {
            return Some(path);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        for anc in exe.ancestors() {
            if anc.extension().and_then(|e| e.to_str()) == Some("app") {
                return Some(anc.to_path_buf());
            }
        }
    }
    None
}

#[derive(Default)]
struct DragIconViewIvars {
    bundle_url: Retained<NSURL>,
    icon: Retained<NSImage>,
    drag_started: std::cell::Cell<bool>,
}

define_class!(
    #[unsafe(super = NSImageView)]
    #[thread_kind = MainThreadOnly]
    #[ivars = DragIconViewIvars]
    struct DragIconView;

    unsafe impl NSObjectProtocol for DragIconView {}

    unsafe impl NSDraggingSource for DragIconView {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn dragging_session_source_operation_mask(
            &self,
            _session: &NSDraggingSession,
            context: NSDraggingContext,
        ) -> NSDragOperation {
            if context == NSDraggingContext::OutsideApplication {
                NSDragOperation::Copy
            } else {
                NSDragOperation::None
            }
        }
    }

    impl DragIconView {
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.ivars().drag_started.set(false);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            if self.ivars().drag_started.get() {
                return;
            }
            self.ivars().drag_started.set(true);

            let url = self.ivars().bundle_url.clone();
            let icon = self.ivars().icon.clone();
            let url_ref: &NSURL = url.as_ref();
            let writer: &ProtocolObject<dyn NSPasteboardWriting> = ProtocolObject::from_ref(url_ref);
            let item =
                NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), writer);
            let frame = self.bounds();
            unsafe {
                item.setDraggingFrame_contents(frame, Some(icon.as_ref()));
            }
            let items = NSArray::from_retained_slice(&[item]);
            let this = ProtocolObject::from_ref(self);
            let _: Retained<NSDraggingSession> =
                NSView::beginDraggingSessionWithItems_event_source(self, &items, event, this);
        }
    }
);

impl DragIconView {
    fn new(
        frame: NSRect,
        url: &NSURL,
        icon: &NSImage,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DragIconViewIvars {
            bundle_url: url.retain(),
            icon: icon.retain(),
            drag_started: std::cell::Cell::new(false),
        });
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        view.setImage(Some(icon));
        view.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        view.setImageAlignment(NSImageAlignment::AlignCenter);
        view
    }
}
