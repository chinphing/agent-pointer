//! Offline Ed25519 license signing tool (private key never ships in pointer-server).

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{TimeZone, Utc};
use clap::{Parser, Subcommand};
use ed25519_dalek::{Signer, SigningKey};
use pointer_core::license::{
    binding_for_current_host, binding_from_explicit_machine_id, LicenseClaims,
    MachineIdentityView,
};
use rand::rngs::OsRng;
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "license-gen", about = "Sign pointer-server standalone licenses")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a new Ed25519 keypair (hex + base64 files).
    GenKeypair {
        #[arg(long, default_value = "license.key")]
        private_key: PathBuf,
        #[arg(long, default_value = "license.pub")]
        public_key: PathBuf,
    },
    /// Sign a license for a customer.
    Sign {
        #[arg(long)]
        private_key: PathBuf,
        #[arg(long)]
        customer_id: String,
        /// Expiry as RFC3339 or YYYY-MM-DD.
        #[arg(long)]
        expires: String,
        #[arg(long, value_delimiter = ',')]
        features: Vec<String>,
        #[arg(long)]
        max_seats: Option<u32>,
        /// Bind license to this machine's v2 fingerprint (AWS/Azure/board drift anchors).
        #[arg(long)]
        bind_machine: bool,
        /// Override binding token from `./pointer-server --machine-id` (fp1:…) or legacy os id.
        #[arg(long)]
        machine_id: Option<String>,
        /// Full identity JSON from `./pointer-server --machine-id-json` (includes drift anchors).
        #[arg(long)]
        machine_id_json: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::GenKeypair {
            private_key,
            public_key,
        } => gen_keypair(&private_key, &public_key),
        Command::Sign {
            private_key,
            customer_id,
            expires,
            features,
            max_seats,
            bind_machine,
            machine_id,
            machine_id_json,
        } => sign_license(
            &private_key,
            &customer_id,
            &expires,
            &features,
            max_seats,
            bind_machine,
            machine_id,
            machine_id_json,
        ),
    }
}

fn gen_keypair(private_path: &PathBuf, public_path: &PathBuf) -> Result<()> {
    let mut csprng = OsRng;
    let signing = SigningKey::generate(&mut csprng);
    let verifying = signing.verifying_key();
    let priv_b64 = URL_SAFE_NO_PAD.encode(signing.to_bytes());
    let pub_b64 = URL_SAFE_NO_PAD.encode(verifying.to_bytes());
    fs::write(private_path, format!("{priv_b64}\n"))
        .with_context(|| format!("write {}", private_path.display()))?;
    fs::write(public_path, format!("{pub_b64}\n"))
        .with_context(|| format!("write {}", public_path.display()))?;
    println!("private key: {}", private_path.display());
    println!("public key:  {}", public_path.display());
    println!("embed public key in pointer-core/license.pub for release builds");
    Ok(())
}

fn load_signing_key(path: &PathBuf) -> Result<SigningKey> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("read private key {}", path.display()))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(raw.trim())
        .context("decode private key base64")?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow!("private key must be 32 bytes"))?;
    Ok(SigningKey::from_bytes(&arr))
}

fn parse_expires(raw: &str) -> Result<i64> {
    let trimmed = raw.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return Ok(dt.timestamp());
    }
    if let Ok(nd) = chrono::NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        let dt = nd
            .and_hms_opt(23, 59, 59)
            .ok_or_else(|| anyhow!("invalid date"))?;
        return Ok(Utc.from_utc_datetime(&dt).timestamp());
    }
    Err(anyhow!(
        "expires must be RFC3339 or YYYY-MM-DD, got {trimmed}"
    ))
}

fn sign_license(
    private_key: &PathBuf,
    customer_id: &str,
    expires: &str,
    features: &[String],
    max_seats: Option<u32>,
    bind_machine: bool,
    machine_id: Option<String>,
    machine_id_json: Option<PathBuf>,
) -> Result<()> {
    if machine_id.is_some() && machine_id_json.is_some() {
        return Err(anyhow!("use only one of --machine-id or --machine-id-json"));
    }
    let signing = load_signing_key(private_key)?;
    let (bound_id, machine_board_fp, machine_cloud_fp) =
        if let Some(json_path) = machine_id_json {
            let text = fs::read_to_string(&json_path)
                .with_context(|| format!("read {}", json_path.display()))?;
            let view: MachineIdentityView =
                serde_json::from_str(&text).context("parse machine identity json")?;
            (
                Some(view.machine_id),
                view.machine_board_fp,
                view.machine_cloud_fp,
            )
        } else if let Some(provided) = machine_id {
            let (mid, board, cloud) = binding_from_explicit_machine_id(&provided)?;
            (Some(mid), board, cloud)
        } else if bind_machine {
            let (mid, board, cloud) = binding_for_current_host()?;
            (Some(mid), board, cloud)
        } else {
            (None, None, None)
        };

    let claims = LicenseClaims {
        customer_id: customer_id.trim().to_string(),
        expires_at: parse_expires(expires)?,
        features: features
            .iter()
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty())
            .collect(),
        max_seats,
        machine_id: bound_id,
        machine_board_fp,
        machine_cloud_fp,
    };
    if claims.customer_id.is_empty() {
        return Err(anyhow!("customer_id is required"));
    }
    let payload = serde_json::to_vec(&claims)?;
    let signature = signing.sign(&payload);
    let license_key = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(&payload),
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    );
    println!("{license_key}");
    Ok(())
}
