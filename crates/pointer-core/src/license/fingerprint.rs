//! Machine binding fingerprints for standalone licenses (v1 strict + drift anchors).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;

const FINGERPRINT_PREFIX: &str = "fp1:";
const FINGERPRINT_SALT: &str = "pointer-server-license-v1";

/// Raw signals collected from the host (best-effort; fields may be empty).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineFactors {
    pub os_id: String,
    pub board_uuid: String,
    pub cloud_provider: String,
    pub cloud_instance_id: String,
}

/// Derived SHA-256 fingerprints for license binding and drift checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineFingerprints {
    /// Primary binding token (`fp1:{64-hex}`).
    pub strict: String,
    /// Drift anchor when OS installation ID changes (board UUID).
    pub board: Option<String>,
    /// Drift anchor for cloud VMs (provider + instance id).
    pub cloud: Option<String>,
}

/// JSON view for `--machine-id-json` and license issuance workflows.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineIdentityView {
    pub os_id: String,
    pub board_uuid: Option<String>,
    pub cloud_provider: Option<String>,
    pub cloud_instance_id: Option<String>,
    /// Value to embed in `LicenseClaims.machine_id` for new licenses.
    pub machine_id: String,
    pub machine_board_fp: Option<String>,
    pub machine_cloud_fp: Option<String>,
}

impl MachineFingerprints {
    pub fn from_factors(factors: &MachineFactors) -> Self {
        let strict = format!(
            "{FINGERPRINT_PREFIX}{}",
            hash_parts(&[
                ("os", normalize_factor(&factors.os_id)),
                ("board", normalize_factor(&factors.board_uuid),),
                (
                    "cloud",
                    cloud_token(&factors.cloud_provider, &factors.cloud_instance_id),
                ),
            ])
        );
        let board = {
            let token = normalize_factor(&factors.board_uuid);
            if token.is_empty() {
                None
            } else {
                Some(hash_parts(&[("board", token)]))
            }
        };
        let cloud = {
            let token = cloud_token(&factors.cloud_provider, &factors.cloud_instance_id);
            if token.is_empty() {
                None
            } else {
                Some(hash_parts(&[("cloud", token)]))
            }
        };
        Self {
            strict,
            board,
            cloud,
        }
    }
}

impl MachineIdentityView {
    pub fn from_factors(factors: MachineFactors) -> Self {
        let fps = MachineFingerprints::from_factors(&factors);
        Self {
            os_id: factors.os_id.clone(),
            board_uuid: optional_non_empty(&factors.board_uuid),
            cloud_provider: optional_non_empty(&factors.cloud_provider),
            cloud_instance_id: optional_non_empty(&factors.cloud_instance_id),
            machine_id: fps.strict.clone(),
            machine_board_fp: fps.board.clone(),
            machine_cloud_fp: fps.cloud.clone(),
        }
    }
}

/// Collect machine binding signals from the current host.
pub fn collect_machine_factors() -> Result<MachineFactors> {
    let os_id = machine_uid::get().map_err(|e| anyhow::anyhow!("read os machine id: {e}"))?;
    let board_uuid = read_board_uuid().unwrap_or_default();
    let (cloud_provider, cloud_instance_id) = read_cloud_instance().unwrap_or_default();
    Ok(MachineFactors {
        os_id,
        board_uuid,
        cloud_provider,
        cloud_instance_id,
    })
}

pub fn current_machine_identity() -> Result<MachineIdentityView> {
    Ok(MachineIdentityView::from_factors(collect_machine_factors()?))
}

/// Primary binding token for new licenses (`fp1:…`).
pub fn current_binding_token() -> Result<String> {
    Ok(MachineFingerprints::from_factors(&collect_machine_factors()?).strict)
}

pub fn is_v2_binding_token(value: &str) -> bool {
    value.trim().starts_with(FINGERPRINT_PREFIX)
}

/// Verify license machine binding (legacy raw OS id or v2 fingerprint + drift).
pub fn verify_machine_binding(
    machine_id: Option<&str>,
    machine_board_fp: Option<&str>,
    machine_cloud_fp: Option<&str>,
) -> Result<()> {
    // Unbound licenses must not probe cloud metadata (blocking HTTP inside async startup).
    if machine_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none()
    {
        return Ok(());
    }
    let factors = collect_machine_factors()?;
    verify_machine_binding_with_factors(machine_id, machine_board_fp, machine_cloud_fp, &factors)
}

pub fn verify_machine_binding_with_factors(
    machine_id: Option<&str>,
    machine_board_fp: Option<&str>,
    machine_cloud_fp: Option<&str>,
    factors: &MachineFactors,
) -> Result<()> {
    let Some(bound) = machine_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let current = MachineFingerprints::from_factors(factors);

    if is_v2_binding_token(bound) {
        if current.strict == bound {
            return Ok(());
        }
        if let Some(expected) = machine_board_fp.map(str::trim).filter(|s| !s.is_empty()) {
            if current.board.as_deref() == Some(expected) {
                log::info!("license: machine binding accepted via board drift anchor");
                return Ok(());
            }
        }
        if let Some(expected) = machine_cloud_fp.map(str::trim).filter(|s| !s.is_empty()) {
            if current.cloud.as_deref() == Some(expected) {
                log::info!("license: machine binding accepted via cloud instance drift anchor");
                return Ok(());
            }
        }
        anyhow::bail!(
            "license bound to machine_id={bound} but this host is {} (os_id={})",
            current.strict,
            factors.os_id
        );
    }

    if bound == factors.os_id.trim() {
        return Ok(());
    }
    anyhow::bail!(
        "license bound to legacy machine_id={bound} but this host os_id is {}",
        factors.os_id
    );
}

/// Build v2 binding fields for license claims when binding to current host or explicit token.
pub fn binding_for_current_host() -> Result<(String, Option<String>, Option<String>)> {
    let view = current_machine_identity()?;
    Ok((
        view.machine_id,
        view.machine_board_fp,
        view.machine_cloud_fp,
    ))
}

/// Parse explicit `--machine-id` for remote signing.
pub fn binding_from_explicit_machine_id(
    raw: &str,
) -> Result<(String, Option<String>, Option<String>)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("machine id is empty");
    }
    if is_v2_binding_token(trimmed) {
        return Ok((trimmed.to_string(), None, None));
    }
    Ok((trimmed.to_string(), None, None))
}

fn hash_parts(parts: &[(&str, String)]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(FINGERPRINT_SALT.as_bytes());
    hasher.update(b"|");
    for (key, value) in parts {
        if value.is_empty() {
            continue;
        }
        hasher.update(key.as_bytes());
        hasher.update(b"=");
        hasher.update(value.as_bytes());
        hasher.update(b"|");
    }
    hex::encode(hasher.finalize())
}

fn normalize_factor(raw: &str) -> String {
    raw.trim()
        .trim_matches('{')
        .trim_matches('}')
        .to_ascii_lowercase()
}

fn cloud_token(provider: &str, instance_id: &str) -> String {
    let p = normalize_factor(provider);
    let id = instance_id.trim();
    if p.is_empty() || id.is_empty() {
        String::new()
    } else {
        format!("{p}:{id}")
    }
}

fn optional_non_empty(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn read_board_uuid() -> Result<String> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(raw) = std::fs::read_to_string("/sys/class/dmi/id/product_uuid") {
            let uuid = normalize_factor(&raw);
            if is_valid_board_uuid(&uuid) {
                return Ok(uuid);
            }
        }
        if let Ok(output) = std::process::Command::new("dmidecode")
            .args(["-s", "system-uuid"])
            .output()
        {
            if output.status.success() {
                let uuid = normalize_factor(&String::from_utf8_lossy(&output.stdout));
                if is_valid_board_uuid(&uuid) {
                    return Ok(uuid);
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(id) = machine_uid::get() {
            let uuid = normalize_factor(&id);
            if is_valid_board_uuid(&uuid) {
                return Ok(uuid);
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        if let Ok(output) = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_ComputerSystemProduct).UUID",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            if output.status.success() {
                let uuid = normalize_factor(&String::from_utf8_lossy(&output.stdout));
                if is_valid_board_uuid(&uuid) {
                    return Ok(uuid);
                }
            }
        }
    }
    Ok(String::new())
}

fn is_valid_board_uuid(uuid: &str) -> bool {
    !uuid.is_empty()
        && uuid != "00000000-0000-0000-0000-000000000000"
        && uuid != "ffffffff-ffff-ffff-ffff-ffffffffffff"
}

fn read_cloud_instance() -> Result<(String, String)> {
    if let Some(id) = fetch_aws_instance_id() {
        return Ok(("aws".into(), id));
    }
    if let Some((provider, id)) = fetch_azure_instance() {
        return Ok((provider, id));
    }
    Ok((String::new(), String::new()))
}

fn metadata_client() -> Option<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(400))
        .build()
        .ok()
}

fn fetch_aws_instance_id() -> Option<String> {
    let client = metadata_client()?;
    let text = client
        .get("http://169.254.169.254/latest/meta-data/instance-id")
        .header("User-Agent", "pointer-server")
        .send()
        .ok()?
        .text()
        .ok()?;
    let id = text.trim();
    if id.is_empty() || !id.starts_with("i-") {
        return None;
    }
    Some(id.to_string())
}

fn fetch_azure_instance() -> Option<(String, String)> {
    let client = metadata_client()?;
    let text = client
        .get("http://169.254.169.254/metadata/instance?api-version=2021-02-01")
        .header("Metadata", "true")
        .header("User-Agent", "pointer-server")
        .send()
        .ok()?
        .text()
        .ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let vm_id = json
        .pointer("/compute/vmId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    Some(("azure".into(), vm_id.to_string()))
}

mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_fingerprint_is_stable() {
        let factors = MachineFactors {
            os_id: "abc123".into(),
            board_uuid: "board-1".into(),
            cloud_provider: "aws".into(),
            cloud_instance_id: "i-0123456789abcdef0".into(),
        };
        let a = MachineFingerprints::from_factors(&factors);
        let b = MachineFingerprints::from_factors(&factors);
        assert_eq!(a, b);
        assert!(a.strict.starts_with(FINGERPRINT_PREFIX));
    }

    #[test]
    fn drift_anchors_survive_os_id_change() {
        let original = MachineFactors {
            os_id: "os-old".into(),
            board_uuid: "board-1".into(),
            cloud_provider: "aws".into(),
            cloud_instance_id: "i-abc".into(),
        };
        let original_fp = MachineFingerprints::from_factors(&original);

        let after_reinstall = MachineFactors {
            os_id: "os-new".into(),
            ..original.clone()
        };
        let after_fp = MachineFingerprints::from_factors(&after_reinstall);
        assert_ne!(original_fp.strict, after_fp.strict);
        assert_eq!(original_fp.board, after_fp.board);
        assert_eq!(original_fp.cloud, after_fp.cloud);

        verify_machine_binding_with_factors(
            Some(&original_fp.strict),
            original_fp.board.as_deref(),
            original_fp.cloud.as_deref(),
            &after_reinstall,
        )
        .expect("board/cloud drift anchors should pass");
    }
}
