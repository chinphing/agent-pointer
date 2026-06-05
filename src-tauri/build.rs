fn main() {
    // Master source + bundle outputs — force relink when icons change.
    for path in [
        "icons/icon.png",
        "icons/icon.ico",
        "icons/icon.icns",
        "icons/32x32.png",
        "icons/128x128.png",
        "icons/128x128@2x.png",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    emit_app_version_from_tauri_conf();
    tauri_build::build()
}

fn emit_app_version_from_tauri_conf() {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let conf_path = manifest_dir.join("tauri.conf.json");
    println!("cargo:rerun-if-changed={}", conf_path.display());
    let Ok(raw) = std::fs::read_to_string(&conf_path) else {
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
