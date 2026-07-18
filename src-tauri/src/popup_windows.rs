//! Allow trusted popup windows (e.g. WeCom bot QR auth) opened via `window.open`.

use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(target_os = "macos")]
use tauri::WindowEvent;

use tauri::{
    utils::config::WebviewUrl,
    webview::{NewWindowFeatures, NewWindowResponse, WebviewWindowBuilder},
    App, Url, Wry,
};

static POPUP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn is_allowed_popup_url(url: &Url) -> bool {
    matches!(url.host_str(), Some("work.weixin.qq.com"))
}

fn handle_new_window(
    app_handle: &tauri::AppHandle,
    url: Url,
    features: NewWindowFeatures,
) -> NewWindowResponse<Wry> {
    if !is_allowed_popup_url(&url) {
        log::warn!("popup blocked: disallowed url {url}");
        return NewWindowResponse::Deny;
    }

    let label = format!("popup-{}", POPUP_COUNTER.fetch_add(1, Ordering::Relaxed));
    match WebviewWindowBuilder::new(app_handle, &label, WebviewUrl::External(url))
        .window_features(features)
        .title("企业微信授权")
        .decorations(true)
        .resizable(false)
        .build()
    {
        Ok(window) => {
            log::info!("popup window created: {label}");
            NewWindowResponse::Create { window }
        }
        Err(err) => {
            log::warn!("popup window create failed ({label}): {err}");
            NewWindowResponse::Deny
        }
    }
}

/// Create the main window from config with a popup handler for WeCom auth.
pub fn create_main_window(app: &App) -> Result<(), String> {
    let main_config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .ok_or_else(|| "main window config not found".to_string())?
        .clone();

    let app_handle = app.handle().clone();
    // Composer OS file drop needs drag_drop_enabled=true (tauri.conf default).
    // false disables native handler; HTML5 drop is unreliable for Finder/Explorer in WebView.
    log::info!(
        "main window drag_drop_enabled={}",
        main_config.drag_drop_enabled
    );
    WebviewWindowBuilder::from_config(app, &main_config)
        .map_err(|e| format!("main window builder: {e}"))?
        .on_new_window(move |url, features| handle_new_window(&app_handle, url, features))
        .build()
        .map_err(|e| format!("main window build: {e}"))?;

    log::info!("main window created with popup handler");
    Ok(())
}
