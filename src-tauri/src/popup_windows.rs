//! Main window navigation / popup policy:
//! - Keep SPA / local origins in the webview.
//! - Open other http(s) links in the OS default browser.
//! - Allow WeCom bot QR auth popups (`work.weixin.qq.com`) as in-app windows.

use std::sync::atomic::{AtomicU64, Ordering};

use pointer_core::platform_auth::open_url_in_browser;
use tauri::{
    utils::config::WebviewUrl,
    webview::{NewWindowFeatures, NewWindowResponse, WebviewWindowBuilder},
    App, Url, Wry,
};

static POPUP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn is_allowed_popup_url(url: &Url) -> bool {
    matches!(url.host_str(), Some("work.weixin.qq.com"))
}

fn is_app_internal_navigation(url: &Url) -> bool {
    match url.scheme() {
        "http" | "https" => matches!(
            url.host_str(),
            Some("localhost" | "127.0.0.1" | "tauri.localhost")
        ),
        // Custom protocols (tauri/asset), blank pages, etc. stay in-webview.
        _ => true,
    }
}

fn open_http_url_in_browser(url: &Url) {
    let url_str = url.as_str();
    match open_url_in_browser(url_str) {
        Ok(()) => log::info!("opened external url in default browser: {url_str}"),
        Err(err) => log::warn!("failed to open external url in browser ({url_str}): {err:#}"),
    }
}

fn handle_navigation(url: &Url) -> bool {
    if is_app_internal_navigation(url) {
        return true;
    }
    if matches!(url.scheme(), "http" | "https") {
        open_http_url_in_browser(url);
        return false;
    }
    log::warn!("blocked unexpected webview navigation: {url}");
    false
}

fn handle_new_window(
    app_handle: &tauri::AppHandle,
    url: Url,
    features: NewWindowFeatures,
) -> NewWindowResponse<Wry> {
    if is_allowed_popup_url(&url) {
        let label = format!("popup-{}", POPUP_COUNTER.fetch_add(1, Ordering::Relaxed));
        return match WebviewWindowBuilder::new(app_handle, &label, WebviewUrl::External(url))
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
        };
    }

    if matches!(url.scheme(), "http" | "https") {
        open_http_url_in_browser(&url);
        return NewWindowResponse::Deny;
    }

    log::warn!("popup blocked: disallowed url {url}");
    NewWindowResponse::Deny
}

/// Create the main window from config with external-link + WeCom popup handlers.
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
        .on_navigation(|url| handle_navigation(url))
        .on_new_window(move |url, features| handle_new_window(&app_handle, url, features))
        .build()
        .map_err(|e| format!("main window build: {e}"))?;

    log::info!("main window created with external-link and popup handlers");
    Ok(())
}
