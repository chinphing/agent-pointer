//! Ed25519 offline license verification for standalone pointer-server deployments.

mod verify;

pub use verify::{
    active_license_claims, active_license_status_view, current_machine_id, feature_enabled, license_status,
    reload_license_from_env, validate_license_at_startup, LicenseClaims, LicenseStatus,
    LicenseStatusView, LicenseVerifier,
};
