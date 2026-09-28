//! Build edition: `official` (ships Pointer's own control-plane defaults) vs unset.
//!
//! `POINTER_EDITION` is a **packaging flavour**, not a gate: whether a build can
//! reach a control plane is decided at runtime by
//! `platform_endpoints::control_plane_bound()` / `deployment_mode::is_standalone()`.
//! Set at compile time via `POINTER_EDITION` when invoking cargo, or override at runtime.

const ENV_EDITION: &str = "POINTER_EDITION";

/// Effective edition string: `official`, or empty (unset).
pub fn edition() -> String {
    if let Ok(raw) = std::env::var(ENV_EDITION) {
        let trimmed = raw.trim().to_ascii_lowercase();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    option_env!("POINTER_EDITION")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Official build: ships Pointer's production control-plane defaults.
pub fn is_official() -> bool {
    edition() == "official"
}

/// Log edition once after config env vars are applied.
pub fn init_from_env() {
    let value = edition();
    if value.is_empty() {
        log::info!("edition: unset (personal defaults)");
    } else {
        log::info!("edition: {value}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_is_not_official() {
        // Compile-time POINTER_EDITION is usually empty in `cargo test`.
        if std::env::var(ENV_EDITION).ok().filter(|v| !v.trim().is_empty()).is_some() {
            return;
        }
        if option_env!("POINTER_EDITION")
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
        {
            return;
        }
        assert!(!is_official());
        assert!(edition().is_empty());
    }
}
