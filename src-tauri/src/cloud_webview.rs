//! Cloud agent WebView windows (multi-window remote pointer-app UI).

use pointer_core::chat_service::AppState;
use pointer_core::platform_agents::{create_agent_oauth_code, get_agent, CloudAgent};
use std::sync::Arc;
use tauri::utils::config::WebviewUrl;
use tauri::{AppHandle, Manager, Url, WebviewWindowBuilder};

const WINDOW_WIDTH: f64 = 1280.0;
const WINDOW_HEIGHT: f64 = 800.0;

fn cloud_window_label(agent_id: &str) -> String {
    format!("cloud-agent-{}", agent_id.trim())
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn build_console_url(base: &str, code: &str, state: &str) -> Result<Url, String> {
    let base = base.trim();
    let sep = if base.contains('?') { '&' } else { '?' };
    let url_str = format!(
        "{base}{sep}code={}&state={}",
        urlencoding_encode(code),
        urlencoding_encode(state)
    );
    Url::parse(&url_str).map_err(|_| "控制台地址无效".to_string())
}

pub fn is_cloud_agent_window_open(app: &AppHandle, agent_id: &str) -> bool {
    app.get_webview_window(&cloud_window_label(agent_id))
        .is_some()
}

pub fn focus_cloud_agent_window(app: &AppHandle, agent_id: &str) -> Result<(), String> {
    let label = cloud_window_label(agent_id);
    let win = app
        .get_webview_window(&label)
        .ok_or_else(|| "云主机窗口未打开".to_string())?;
    win.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn close_cloud_agent_window(app: &AppHandle, agent_id: &str) -> Result<(), String> {
    let label = cloud_window_label(agent_id);
    if let Some(win) = app.get_webview_window(&label) {
        win.close().map_err(|e| e.to_string())?;
        log::info!("cloud_webview: closed {label}");
    }
    Ok(())
}

pub async fn open_cloud_agent_window(
    app: AppHandle,
    state: Arc<AppState>,
    agent_id: String,
) -> Result<(), String> {
    state
        .platform_auth
        .refresh_if_needed()
        .await
        .map_err(|e| e.to_string())?;
    state
        .platform_auth
        .ensure_llm_allowed()
        .await
        .map_err(|e| e.to_string())?;

    let oauth = create_agent_oauth_code(state.platform_auth.as_ref(), &agent_id)
        .await
        .map_err(|e| e.to_string())?;
    let agent = get_agent(state.platform_auth.as_ref(), &agent_id)
        .await
        .map_err(|e| e.to_string())?;
    let base = agent
        .console_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "智能体尚未就绪，无法打开".to_string())?;
    let target = build_console_url(base, &oauth.code, &oauth.state)?;

    let label = cloud_window_label(&agent_id);
    if let Some(existing) = app.get_webview_window(&label) {
        let js = format!(
            "window.location.replace({});",
            serde_json::to_string(target.as_str()).unwrap_or_default()
        );
        existing.eval(&js).map_err(|e| e.to_string())?;
        existing.set_focus().map_err(|e| e.to_string())?;
        log::info!("cloud_webview: navigated existing window {label}");
        return Ok(());
    }

    WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(target))
        .title(window_title(&agent))
        .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .build()
        .map_err(|e| format!("创建云主机窗口失败: {e}"))?;
    log::info!("cloud_webview: opened {label}");
    Ok(())
}

fn window_title(agent: &CloudAgent) -> String {
    let region = agent
        .server_region
        .as_ref()
        .map(|r| r.name_zh.as_str())
        .unwrap_or("云主机");
    format!("Pointer · {region}")
}
