//! pointer-server / SOM API base URLs.
//!
//! - **Unset edition** (local `tauri dev` / default): production domains unless standalone.
//! - **Official** (`POINTER_EDITION=official`): same production domains.
//! - **Community** (`POINTER_EDITION=community`): empty defaults; set `POINTER_*` to opt in.
//! - **Standalone**: empty unless env is set.
//! - Any build can override with `POINTER_*` / `COMPUTER_ANNOTATE_API_BASE`.

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

fn env_or_standalone_empty(key: &str, platform_default: &str) -> String {
    if let Ok(raw) = std::env::var(key) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if crate::deployment_mode::is_standalone() || crate::edition::is_community() {
        log::warn!(
            "platform_endpoints: {key} not set (standalone or community edition); related platform features disabled"
        );
        return String::new();
    }
    platform_default.to_string()
}

pub fn api_base() -> String {
    env_or_standalone_empty("POINTER_API_BASE", DEFAULT_API_BASE)
}

pub fn web_base() -> String {
    env_or_standalone_empty("POINTER_WEB_BASE", DEFAULT_WEB_BASE)
}

pub fn oauth_client_id() -> String {
    std::env::var("POINTER_OAUTH_CLIENT_ID").unwrap_or_else(|_| DEFAULT_OAUTH_CLIENT_ID.to_string())
}

pub fn annotate_api_base() -> String {
    env_or_standalone_empty("COMPUTER_ANNOTATE_API_BASE", DEFAULT_ANNOTATE_API_BASE)
}
