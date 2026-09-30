use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    build_cjk_fts_extension();

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let tauri_conf = manifest_dir.join("../../src-tauri/tauri.conf.json");
    println!("cargo:rerun-if-changed={}", tauri_conf.display());
    emit_app_version_from_tauri_conf(&tauri_conf);
    emit_license_public_key(&manifest_dir);
    emit_edition();
    emit_platform_domains();
}

fn emit_app_version_from_tauri_conf(path: &Path) {
    let Ok(raw) = fs::read_to_string(path) else {
        return;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    if let Some(version) = parsed.get("version").and_then(|v| v.as_str()) {
        let trimmed = version.trim();
        if !trimmed.is_empty() {
            println!("cargo:rustc-env=POINTER_APP_VERSION={trimmed}");
        }
    }
}

fn emit_edition() {
    println!("cargo:rerun-if-env-changed=POINTER_EDITION");
    if let Ok(raw) = env::var("POINTER_EDITION") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            println!("cargo:rustc-env=POINTER_EDITION={trimmed}");
        }
    }
}

/// Control-plane domains for a managed package, baked in at compile time.
///
/// The open-source tree carries no production domain: a managed build must
/// supply them here, and a build that does not stays unbound (standalone).
fn emit_platform_domains() {
    const DOMAINS: [(&str, &str); 3] = [
        ("POINTER_API_BASE", "POINTER_BUILTIN_API_BASE"),
        ("POINTER_WEB_BASE", "POINTER_BUILTIN_WEB_BASE"),
        (
            "COMPUTER_ANNOTATE_API_BASE",
            "POINTER_BUILTIN_ANNOTATE_API_BASE",
        ),
    ];

    // `official` is the legacy value of POINTER_EDITION and still means managed.
    let managed = env::var("POINTER_EDITION")
        .map(|raw| {
            let value = raw.trim().to_ascii_lowercase();
            value == "managed" || value == "official"
        })
        .unwrap_or(false);

    let mut missing: Vec<&str> = Vec::new();
    for (key, rustc_env) in DOMAINS {
        println!("cargo:rerun-if-env-changed={key}");
        let value = env::var(key).unwrap_or_default();
        let value = value.trim();
        if value.is_empty() {
            missing.push(key);
        }
        println!("cargo:rustc-env={rustc_env}={value}");
    }

    if managed && !missing.is_empty() {
        panic!(
            "pointer-core: a managed build must inject its control-plane domains, but these are missing or empty: {}. \
             The open-source tree no longer hardcodes them; see docs/contributing/editions.md.",
            missing.join(", ")
        );
    }
}

/// Embed `license.pub` (base64 Ed25519 public key) for standalone license verification.
fn emit_license_public_key(manifest_dir: &Path) {
    let key_path = manifest_dir.join("license.pub");
    println!("cargo:rerun-if-changed={}", key_path.display());
    let Ok(raw) = fs::read_to_string(&key_path) else {
        println!(
            "cargo:warning=pointer-core: {} missing; standalone license verification requires POINTER_LICENSE_PUBLIC_KEY at runtime",
            key_path.display()
        );
        return;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        println!(
            "cargo:warning=pointer-core: {} is empty",
            key_path.display()
        );
        return;
    }
    println!("cargo:rustc-env=POINTER_LICENSE_PUBLIC_KEY={trimmed}");
}

/// Statically link sqlite-cjk-fts into pointer-core (no runtime DLL load on Windows).
fn build_cjk_fts_extension() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let src = manifest_dir.join("vendor/sqlite-cjk-fts/cjk_tokenizer.c");
    let include_dir = manifest_dir.join("vendor/sqlite-cjk-fts");
    let sqlite3_include = sqlite3_include_dir();
    println!("cargo:rerun-if-changed={}", src.display());
    println!(
        "cargo:rerun-if-changed={}",
        include_dir.join("sqlite3ext.h").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        sqlite3_include.join("sqlite3.h").display()
    );

    let mut build = cc::Build::new();
    build
        .file(&src)
        .include(&include_dir)
        .include(&sqlite3_include)
        .opt_level(2);
    if !cfg!(target_os = "windows") {
        build.flag("-fPIC");
    }
    build.compile("cjkfts");
}

/// Bundled `sqlite3.h` from libsqlite3-sys (same as rusqlite); Windows has no system copy.
fn sqlite3_include_dir() -> PathBuf {
    env::var("DEP_SQLITE3_INCLUDE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            panic!(
                "DEP_SQLITE3_INCLUDE is not set; ensure libsqlite3-sys is a direct dependency with bundled"
            )
        })
}
