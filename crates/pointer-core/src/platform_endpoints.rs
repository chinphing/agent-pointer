//! 官网 / SOM 联调地址。
//!
//! - **Release**（`tauri build`）：默认生产域名，用户无需配置环境变量。
//! - **Debug**（`tauri dev`）：默认本机联调地址。
//! - 任意构建均可通过 `POINTER_*` / `COMPUTER_ANNOTATE_API_BASE` 覆盖。

#[cfg(debug_assertions)]
pub const DEFAULT_API_BASE: &str = "https://pointer-api.readflowai.com";
#[cfg(not(debug_assertions))]
pub const DEFAULT_API_BASE: &str = "https://pointer-api.readflowai.com";

#[cfg(debug_assertions)]
pub const DEFAULT_WEB_BASE: &str = "https://pointer.readflowai.com";
#[cfg(not(debug_assertions))]
pub const DEFAULT_WEB_BASE: &str = "https://pointer.readflowai.com";

pub const DEFAULT_OAUTH_CLIENT_ID: &str = "pointer-desktop";

#[cfg(debug_assertions)]
pub const DEFAULT_ANNOTATE_API_BASE: &str = "https://pointer-som.readflowai.com";
#[cfg(not(debug_assertions))]
pub const DEFAULT_ANNOTATE_API_BASE: &str = "https://pointer-som.readflowai.com";

pub fn api_base() -> String {
    std::env::var("POINTER_API_BASE").unwrap_or_else(|_| DEFAULT_API_BASE.to_string())
}

pub fn web_base() -> String {
    std::env::var("POINTER_WEB_BASE").unwrap_or_else(|_| DEFAULT_WEB_BASE.to_string())
}

pub fn oauth_client_id() -> String {
    std::env::var("POINTER_OAUTH_CLIENT_ID")
        .unwrap_or_else(|_| DEFAULT_OAUTH_CLIENT_ID.to_string())
}

pub fn annotate_api_base() -> String {
    std::env::var("COMPUTER_ANNOTATE_API_BASE")
        .unwrap_or_else(|_| DEFAULT_ANNOTATE_API_BASE.to_string())
}
