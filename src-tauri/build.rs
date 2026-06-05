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
    tauri_build::build()
}
