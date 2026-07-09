//! Ed25519 offline license verification for standalone pointer-server deployments.

mod fingerprint;
mod verify;

pub use fingerprint::{
    binding_for_current_host, binding_from_explicit_machine_id, collect_machine_factors,
    current_binding_token, current_machine_identity, is_v2_binding_token, verify_machine_binding,
    MachineFactors, MachineFingerprints, MachineIdentityView,
};
pub use verify::{
    active_license_claims, active_license_status_view, current_machine_id, feature_enabled,
    license_status, reload_license_from_env, validate_license_at_startup, LicenseClaims,
    LicenseStatus, LicenseStatusView, LicenseVerifier,
};
