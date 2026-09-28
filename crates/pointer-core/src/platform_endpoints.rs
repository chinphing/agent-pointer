//! pointer-server / SOM API base URLs.
//!
//! - **Official build** (`POINTER_EDITION=official`): production domains by default.
//! - **Any other build**: unbound unless `POINTER_*` is set, i.e. standalone.
//! - **Standalone / unbound**: empty, so related platform features stay disabled.
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

/// Resolve one platform base URL: an explicit env var wins, otherwise only an
/// official build falls back to the production default. Anything else is unbound
/// and behaves as standalone.
fn resolve_platform_base(key: &str, platform_default: &str) -> String {
    if let Ok(raw) = std::env::var(key) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if crate::edition::is_official() {
        return platform_default.to_string();
    }
    String::new()
}

/// Whether this process has a control plane to talk to.
pub fn control_plane_bound() -> bool {
    !resolve_platform_base("POINTER_API_BASE", DEFAULT_API_BASE).is_empty()
        || !resolve_platform_base("POINTER_WEB_BASE", DEFAULT_WEB_BASE).is_empty()
}

pub fn api_base() -> String {
    resolve_platform_base("POINTER_API_BASE", DEFAULT_API_BASE)
}

pub fn web_base() -> String {
    resolve_platform_base("POINTER_WEB_BASE", DEFAULT_WEB_BASE)
}

pub fn oauth_client_id() -> String {
    std::env::var("POINTER_OAUTH_CLIENT_ID").unwrap_or_else(|_| DEFAULT_OAUTH_CLIENT_ID.to_string())
}

pub fn annotate_api_base() -> String {
    resolve_platform_base("COMPUTER_ANNOTATE_API_BASE", DEFAULT_ANNOTATE_API_BASE)
}
