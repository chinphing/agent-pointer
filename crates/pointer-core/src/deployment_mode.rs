//! Deployment mode: `platform` (bound to a control plane) vs `standalone` (no control plane).
//!
//! Standalone is the default when nothing binds a control plane, so a local dev run
//! or a personal build behaves like a self-hosted install: local accounts and local
//! model keys, no login wall. `POINTER_DEPLOYMENT_MODE` overrides explicitly.

use std::sync::OnceLock;

const ENV_DEPLOYMENT_MODE: &str = "POINTER_DEPLOYMENT_MODE";

static MODE: OnceLock<String> = OnceLock::new();

fn normalized_mode() -> &'static str {
    MODE.get_or_init(|| {
        let explicit = std::env::var(ENV_DEPLOYMENT_MODE)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        derive_mode(&explicit, crate::platform_endpoints::control_plane_bound()).to_string()
    })
}

/// `POINTER_DEPLOYMENT_MODE` wins when set; otherwise an unbound process is standalone.
fn derive_mode(explicit: &str, control_plane_bound: bool) -> &'static str {
    match explicit {
        "standalone" => "standalone",
        "platform" => "platform",
        _ if control_plane_bound => "platform",
        _ => "standalone",
    }
}

/// Effective deployment mode string (`"platform"` or `"standalone"`).
pub fn deployment_mode() -> &'static str {
    normalized_mode()
}

/// Whether the process runs standalone: no control plane to sign in to.
pub fn is_standalone() -> bool {
    deployment_mode() == "standalone"
}

/// Whether the process runs bound to a control plane.
pub fn is_platform() -> bool {
    !is_standalone()
}

/// Called once at startup after server config env vars are applied.
pub fn init_from_env() {
    let _ = normalized_mode();
    log::info!(
        "deployment_mode: {} (control_plane_bound={})",
        deployment_mode(),
        crate::platform_endpoints::control_plane_bound()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_mode_wins() {
        assert_eq!(derive_mode("standalone", true), "standalone");
        assert_eq!(derive_mode("platform", false), "platform");
    }

    #[test]
    fn unbound_defaults_to_standalone() {
        assert_eq!(derive_mode("", false), "standalone");
        assert_eq!(derive_mode("", true), "platform");
    }
}
