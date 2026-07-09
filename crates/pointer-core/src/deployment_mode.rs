//! pointer-server deployment mode: platform (official OAuth) vs standalone (self-hosted).

use std::sync::OnceLock;

const ENV_DEPLOYMENT_MODE: &str = "POINTER_DEPLOYMENT_MODE";

static MODE: OnceLock<String> = OnceLock::new();

fn normalized_mode() -> &'static str {
    MODE.get_or_init(|| {
        std::env::var(ENV_DEPLOYMENT_MODE)
            .unwrap_or_else(|_| "platform".to_string())
            .trim()
            .to_ascii_lowercase()
    })
}

/// Effective deployment mode string (`"platform"` or `"standalone"`).
pub fn deployment_mode() -> &'static str {
    normalized_mode()
}

/// Whether the process runs in standalone (self-hosted) mode.
pub fn is_standalone() -> bool {
    deployment_mode() == "standalone"
}

/// Whether the process runs in platform (official OAuth) mode.
pub fn is_platform() -> bool {
    !is_standalone()
}

/// Called once at startup after server config env vars are applied.
pub fn init_from_env() {
    let _ = normalized_mode();
    log::info!("deployment_mode: {}", deployment_mode());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_detection() {
        assert_eq!(deployment_mode(), "platform");
        assert!(!is_standalone());
        assert!(is_platform());
    }
}
