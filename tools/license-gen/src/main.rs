//! Offline Ed25519 license signing tool (private key never ships in pointer-server).

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{TimeZone, Utc};
use clap::{Parser, Subcommand};
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
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
        /// Bind license to this machine's hardware ID (prevents copying to other nodes)
        #[arg(long)]
        bind_machine: bool,
        /// Override machine ID (for offline signing with a known target machine id)
        #[arg(long)]
        machine_id: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct LicenseClaims {
    customer_id: String,
    expires_at: i64,
    #[serde(default)]
    features: Vec<String>,
    #[serde(default)]
    max_seats: Option<u32>,
    #[serde(default)]
    machine_id: Option<String>,
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
        } => sign_license(
            &private_key,
            &customer_id,
            &expires,
            &features,
            max_seats,
            bind_machine,
            machine_id,
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
) -> Result<()> {
    let signing = load_signing_key(private_key)?;
    let machine_id = if let Some(provided) = machine_id {
        Some(provided.trim().to_string())
    } else if bind_machine {
        Some(machine_uid::get().map_err(|e| anyhow!("failed to get machine id: {e}"))?)
    } else {
        None
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
        machine_id,
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
