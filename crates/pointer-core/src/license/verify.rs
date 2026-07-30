//! License key parsing, Ed25519 verification, and startup enforcement.

use super::fingerprint;
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::sync::{OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

const ENV_LICENSE_KEY: &str = "POINTER_LICENSE_KEY";
const ENV_LICENSE_PUBLIC_KEY: &str = "POINTER_LICENSE_PUBLIC_KEY";

/// Resolve the Ed25519 verify key (base64).
///
/// - **debug** (`debug_assertions`): `POINTER_LICENSE_PUBLIC_KEY` env may override
///   the embedded key (local / e2e ephemeral keypairs).
/// - **release**: env override is ignored; only the compile-embedded `license.pub`
///   is used so customers cannot self-sign by swapping the public key at runtime.
fn resolve_license_public_key_b64() -> Result<String> {
    #[cfg(debug_assertions)]
    {
        if let Ok(raw) = std::env::var(ENV_LICENSE_PUBLIC_KEY) {
            let trimmed = raw.trim().to_string();
            if !trimmed.is_empty() {
                log::info!(
                    "license: using {ENV_LICENSE_PUBLIC_KEY} override (debug builds only)"
                );
                return Ok(trimmed);
            }
        }
    }
    #[cfg(not(debug_assertions))]
    {
        if std::env::var_os(ENV_LICENSE_PUBLIC_KEY).is_some_and(|v| !v.is_empty()) {
            log::warn!(
                "license: ignoring {ENV_LICENSE_PUBLIC_KEY} in release builds; \
                 using embedded public key only"
            );
        }
    }

    option_env!("POINTER_LICENSE_PUBLIC_KEY")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            #[cfg(debug_assertions)]
            {
                anyhow!(
                    "license public key not embedded; set {ENV_LICENSE_PUBLIC_KEY} \
                     (debug) or embed crates/pointer-core/license.pub"
                )
            }
            #[cfg(not(debug_assertions))]
            {
                anyhow!(
                    "license public key not embedded in this release binary \
                     (expected crates/pointer-core/license.pub at build time)"
                )
            }
        })
}

/// Claims embedded in a signed license payload (JSON, canonical UTF-8 bytes signed).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LicenseClaims {
    pub customer_id: String,
    pub expires_at: i64,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub max_seats: Option<u32>,
    #[serde(default)]
    pub machine_id: Option<String>,
    /// Drift anchor: board / hardware UUID hash (v2 licenses).
    #[serde(default)]
    pub machine_board_fp: Option<String>,
    /// Drift anchor: cloud provider instance hash (v2 licenses).
    #[serde(default)]
    pub machine_cloud_fp: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LicenseStatus {
    Valid,
    Expired,
    NotConfigured,
    Invalid,
    MachineMismatch,
}

/// Public view returned by HTTP status endpoints (no signature material).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseStatusView {
    pub status: LicenseStatus,
    pub customer_id: Option<String>,
    pub expires_at: Option<i64>,
    pub features: Vec<String>,
    pub max_seats: Option<u32>,
    pub machine_bound: bool,
    pub current_machine_id: Option<String>,
}

/// Read the primary machine binding token (`fp1:…`) for license issuance.
pub fn current_machine_id() -> anyhow::Result<String> {
    fingerprint::current_binding_token().map_err(|e| anyhow::anyhow!("{e}"))
}

#[derive(Clone)]
pub struct LicenseVerifier {
    public_key: VerifyingKey,
}

impl LicenseVerifier {
    /// Build verifier from the compile-time embedded public key (`license.pub`).
    ///
    /// Debug builds may override via `POINTER_LICENSE_PUBLIC_KEY` for local / e2e
    /// self-signed licenses. Release builds ignore that env var and only accept
    /// the embedded key (customers cannot swap the verify key through process env).
    pub fn from_embedded() -> Result<Self> {
        Self::from_base64_public_key(&resolve_license_public_key_b64()?)
    }

    /// Optional constructor for license-gen / tests (32-byte Ed25519 public key, base64).
    pub fn from_base64_public_key(b64: &str) -> Result<Self> {
        let bytes = URL_SAFE_NO_PAD
            .decode(b64.trim())
            .with_context(|| format!("decode license public key base64"))?;
        let key_bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow!("license public key must be 32 bytes"))?;
        Ok(Self {
            public_key: VerifyingKey::from_bytes(&key_bytes)
                .map_err(|e| anyhow!("invalid ed25519 public key: {e}"))?,
        })
    }

    /// Verify license key material and return parsed claims.
    ///
    /// Format: `{base64url(payload_json)}.{base64url(signature)}`
    pub fn verify(&self, key_material: &str) -> Result<LicenseClaims> {
        let trimmed = key_material.trim();
        if trimmed.is_empty() {
            bail!("license key is empty");
        }
        let (payload_b64, sig_b64) = trimmed
            .split_once('.')
            .ok_or_else(|| anyhow!("license key must be payload.signature"))?;
        let payload_bytes = URL_SAFE_NO_PAD
            .decode(payload_b64.trim())
            .context("decode license payload")?;
        let sig_bytes = URL_SAFE_NO_PAD
            .decode(sig_b64.trim())
            .context("decode license signature")?;
        let signature = Signature::from_slice(&sig_bytes)
            .map_err(|e| anyhow!("invalid license signature bytes: {e}"))?;
        self.public_key
            .verify(&payload_bytes, &signature)
            .map_err(|_| anyhow!("license signature verification failed"))?;
        let claims: LicenseClaims =
            serde_json::from_slice(&payload_bytes).context("parse license claims json")?;
        if claims.customer_id.trim().is_empty() {
            bail!("license customer_id is empty");
        }
        Ok(claims)
    }
}

static ACTIVE_CLAIMS: OnceLock<RwLock<Option<LicenseClaims>>> = OnceLock::new();

fn claims_store() -> &'static RwLock<Option<LicenseClaims>> {
    ACTIVE_CLAIMS.get_or_init(|| RwLock::new(None))
}

fn license_key_from_env() -> Option<String> {
    std::env::var(ENV_LICENSE_KEY)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Evaluate claims against wall clock.
pub fn license_status(claims: &LicenseClaims) -> LicenseStatus {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if claims.expires_at < now {
        LicenseStatus::Expired
    } else {
        LicenseStatus::Valid
    }
}

pub fn feature_enabled(claims: &LicenseClaims, feature: &str) -> bool {
    if claims.features.is_empty() {
        return true;
    }
    let needle = feature.trim().to_ascii_lowercase();
    claims
        .features
        .iter()
        .any(|f| f.trim().eq_ignore_ascii_case(&needle))
}

fn store_claims(claims: Option<LicenseClaims>) {
    if let Ok(mut guard) = claims_store().write() {
        *guard = claims;
    }
}

/// Active verified claims after successful startup or reload.
pub fn active_license_claims() -> Option<LicenseClaims> {
    claims_store().read().ok().and_then(|g| g.clone())
}

pub fn active_license_status_view() -> LicenseStatusView {
    match active_license_claims() {
        Some(claims) => {
            let status = license_status(&claims);
            let current_machine_id = fingerprint::current_binding_token().ok();
            LicenseStatusView {
                status,
                customer_id: Some(claims.customer_id.clone()),
                expires_at: Some(claims.expires_at),
                features: claims.features.clone(),
                max_seats: claims.max_seats,
                machine_bound: claims.machine_id.is_some(),
                current_machine_id,
            }
        }
        None => LicenseStatusView {
            status: LicenseStatus::NotConfigured,
            customer_id: None,
            expires_at: None,
            features: Vec::new(),
            max_seats: None,
            machine_bound: false,
            current_machine_id: None,
        },
    }
}

/// Verify configured license key and cache claims. Fails fast on invalid/expired license.
pub fn validate_license_at_startup() -> Result<()> {
    if !crate::deployment_mode::is_standalone() {
        log::info!("license: skipped (platform deployment mode)");
        return Ok(());
    }
    let Some(key) = license_key_from_env() else {
        bail!(
            "standalone mode requires a license key: set [license].key in pointer-server.toml or {ENV_LICENSE_KEY}"
        );
    };
    let verifier = LicenseVerifier::from_embedded()?;
    let claims = verifier.verify(&key)?;
    let status = license_status(&claims);
    if status == LicenseStatus::Expired {
        bail!(
            "license expired for customer_id={} at {}",
            claims.customer_id,
            claims.expires_at
        );
    }
    fingerprint::verify_machine_binding(
        claims.machine_id.as_deref(),
        claims.machine_board_fp.as_deref(),
        claims.machine_cloud_fp.as_deref(),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    store_claims(Some(claims.clone()));
    log::info!(
        "license: valid customer_id={} expires_at={} machine_bound={}",
        claims.customer_id,
        claims.expires_at,
        claims.machine_id.is_some()
    );
    Ok(())
}

/// Re-read `POINTER_LICENSE_KEY` from env and re-verify without process restart.
pub fn reload_license_from_env() -> Result<LicenseStatusView> {
    if !crate::deployment_mode::is_standalone() {
        return Ok(LicenseStatusView {
            status: LicenseStatus::NotConfigured,
            customer_id: None,
            expires_at: None,
            features: Vec::new(),
            max_seats: None,
            machine_bound: false,
            current_machine_id: None,
        });
    }
    let key = license_key_from_env().ok_or_else(|| anyhow!("{ENV_LICENSE_KEY} not set"))?;
    let verifier = LicenseVerifier::from_embedded()?;
    let claims = verifier.verify(&key)?;
    let status = license_status(&claims);
    if status == LicenseStatus::Expired {
        bail!(
            "license expired for customer_id={} at {}",
            claims.customer_id,
            claims.expires_at
        );
    }
    fingerprint::verify_machine_binding(
        claims.machine_id.as_deref(),
        claims.machine_board_fp.as_deref(),
        claims.machine_cloud_fp.as_deref(),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    store_claims(Some(claims.clone()));
    log::info!("license: reloaded customer_id={}", claims.customer_id);
    Ok(LicenseStatusView {
        status,
        customer_id: Some(claims.customer_id),
        expires_at: Some(claims.expires_at),
        features: claims.features,
        max_seats: claims.max_seats,
        machine_bound: claims.machine_id.is_some(),
        current_machine_id: fingerprint::current_binding_token().ok(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    fn sign_claims(signing_key: &SigningKey, claims: &LicenseClaims) -> String {
        let payload = serde_json::to_vec(claims).unwrap();
        let signature = signing_key.sign(&payload);
        format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(&payload),
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        )
    }

    #[test]
    fn verify_round_trip() {
        let mut csprng = OsRng;
        let signing = SigningKey::generate(&mut csprng);
        let verifying = signing.verifying_key();
        let b64 = URL_SAFE_NO_PAD.encode(verifying.to_bytes());
        let verifier = LicenseVerifier::from_base64_public_key(&b64).unwrap();
        let claims = LicenseClaims {
            customer_id: "acme".into(),
            expires_at: i64::MAX / 2,
            features: vec!["chat".into()],
            max_seats: Some(10),
            machine_id: None,
            machine_board_fp: None,
            machine_cloud_fp: None,
        };
        let key = sign_claims(&signing, &claims);
        let parsed = verifier.verify(&key).unwrap();
        assert_eq!(parsed, claims);
    }

    #[test]
    fn verify_v2_machine_binding_with_drift() {
        use super::fingerprint::{
            verify_machine_binding_with_factors, MachineFactors, MachineFingerprints,
        };

        let mut csprng = OsRng;
        let signing = SigningKey::generate(&mut csprng);
        let verifying = signing.verifying_key();
        let b64 = URL_SAFE_NO_PAD.encode(verifying.to_bytes());
        let verifier = LicenseVerifier::from_base64_public_key(&b64).unwrap();

        let host = MachineFactors {
            os_id: "os-original".into(),
            board_uuid: "board-abc".into(),
            cloud_provider: "aws".into(),
            cloud_instance_id: "i-123".into(),
        };
        let fps = MachineFingerprints::from_factors(&host);
        let claims = LicenseClaims {
            customer_id: "acme".into(),
            expires_at: i64::MAX / 2,
            features: vec!["chat".into()],
            max_seats: None,
            machine_id: Some(fps.strict.clone()),
            machine_board_fp: fps.board.clone(),
            machine_cloud_fp: fps.cloud.clone(),
        };
        let key = sign_claims(&signing, &claims);
        let parsed = verifier.verify(&key).unwrap();
        assert_eq!(parsed.machine_id, claims.machine_id);

        let reinstalled = MachineFactors {
            os_id: "os-after-reinstall".into(),
            ..host
        };
        verify_machine_binding_with_factors(
            parsed.machine_id.as_deref(),
            parsed.machine_board_fp.as_deref(),
            parsed.machine_cloud_fp.as_deref(),
            &reinstalled,
        )
        .expect("drift anchors should accept reinstalled os_id");
    }

    #[test]
    #[cfg(debug_assertions)]
    fn debug_env_public_key_overrides_embedded() {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let mut csprng = OsRng;
        let signing = SigningKey::generate(&mut csprng);
        let verifying = signing.verifying_key();
        let b64 = URL_SAFE_NO_PAD.encode(verifying.to_bytes());
        let prev = std::env::var(ENV_LICENSE_PUBLIC_KEY).ok();
        std::env::set_var(ENV_LICENSE_PUBLIC_KEY, &b64);
        let verifier = LicenseVerifier::from_embedded().expect("from_embedded with env");
        let claims = LicenseClaims {
            customer_id: "debug-override".into(),
            expires_at: i64::MAX / 2,
            features: vec!["chat".into()],
            max_seats: None,
            machine_id: None,
            machine_board_fp: None,
            machine_cloud_fp: None,
        };
        let key = sign_claims(&signing, &claims);
        assert_eq!(verifier.verify(&key).unwrap(), claims);
        match prev {
            Some(v) => std::env::set_var(ENV_LICENSE_PUBLIC_KEY, v),
            None => std::env::remove_var(ENV_LICENSE_PUBLIC_KEY),
        }
    }
}
