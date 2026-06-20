use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const SUPPORTED_PLATFORM_KEYS: &[(&str, &str)] = &[
    ("tool_approval_mode", "TOOL_APPROVAL_MODE"),
    ("agent_mode", "AGENT_MODE"),
    ("workspace_root", "WORKSPACE_ROOT"),
    ("lead_agent_id", "LEAD_AGENT_ID"),
    ("active_provider_id", "ACTIVE_PROVIDER_ID"),
    ("model", "MODEL"),
    ("temperature", "TEMPERATURE"),
    ("max_tokens", "MAX_TOKENS"),
    ("context_compression_enabled", "CONTEXT_COMPRESSION_ENABLED"),
    ("context_budget_tokens", "CONTEXT_BUDGET_TOKENS"),
    ("context_keep_recent_user_turns", "CONTEXT_KEEP_RECENT_USER_TURNS"),
    ("context_summary_max_tokens", "CONTEXT_SUMMARY_MAX_TOKENS"),
    ("max_tool_rounds", "MAX_TOOL_ROUNDS"),
    ("max_sub_agent_tool_rounds", "MAX_SUB_AGENT_TOOL_ROUNDS"),
    ("max_sub_agent_spawn_depth", "MAX_SUB_AGENT_SPAWN_DEPTH"),
    ("raw_content_view_enabled", "RAW_CONTENT_VIEW_ENABLED"),
    ("debug_dump_llm_prompts", "DEBUG_DUMP_LLM_PROMPTS"),
    ("debug_menus_enabled", "DEBUG_MENUS_ENABLED"),
    ("user_dynamic_inject_enabled", "USER_DYNAMIC_INJECT_ENABLED"),
    ("computer_human_like", "COMPUTER_HUMAN_LIKE"),
    ("computer_initial_tier", "COMPUTER_INITIAL_TIER"),
    (
        "computer_annotated_screen_view_enabled",
        "COMPUTER_ANNOTATED_SCREEN_VIEW_ENABLED",
    ),
    ("computer_show_monitor_picker", "COMPUTER_SHOW_MONITOR_PICKER"),
    ("web_search_model", "WEB_SEARCH_MODEL"),
    ("dati_api_url", "DATI_API_URL"),
    ("dati_authcode", "DATI_AUTHCODE"),
    ("dati_typeno", "DATI_TYPENO"),
    ("dati_author", "DATI_AUTHOR"),
    ("captcha_slider_offset_px", "CAPTCHA_SLIDER_OFFSET_PX"),
];

fn main() {
    build_cjk_fts_extension();

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let workspace_cfg = manifest_dir.join("../../.pointer-build.toml");
    let local_cfg = manifest_dir.join(".pointer-build.toml");
    let tauri_conf = manifest_dir.join("../../src-tauri/tauri.conf.json");
    println!("cargo:rerun-if-changed={}", workspace_cfg.display());
    println!("cargo:rerun-if-changed={}", local_cfg.display());
    println!("cargo:rerun-if-changed={}", tauri_conf.display());
    emit_app_version_from_tauri_conf(&tauri_conf);

    let Some(config_path) = pick_config_path(&workspace_cfg, &local_cfg) else {
        return;
    };
    println!("cargo:warning=pointer-core: loading {}", config_path.display());
    if let Err(e) = load_build_config(&config_path) {
        println!(
            "cargo:warning=pointer-core: failed to read {}: {e}",
            config_path.display()
        );
    }
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

fn pick_config_path(workspace_cfg: &Path, local_cfg: &Path) -> Option<PathBuf> {
    if workspace_cfg.exists() {
        return Some(workspace_cfg.to_path_buf());
    }
    if local_cfg.exists() {
        return Some(local_cfg.to_path_buf());
    }
    None
}

fn load_build_config(path: &Path) -> anyhow::Result<()> {
    let raw = fs::read_to_string(path)?;
    let parsed: toml::Value = toml::from_str(&raw)?;
    let table = parsed
        .get("platform")
        .and_then(|v| v.as_table())
        .ok_or_else(|| anyhow::anyhow!("missing [platform] table"))?;

    for (key, env_suffix) in SUPPORTED_PLATFORM_KEYS {
        if let Some(v) = table.get(*key) {
            if let Some(serialized) = serialize_toml_scalar(v) {
                println!("cargo:rustc-env=POINTER_BUILD_{env_suffix}={serialized}");
            } else {
                println!(
                    "cargo:warning=pointer-core: skip [platform].{key}, only scalar values are supported"
                );
            }
        }
    }
    Ok(())
}

fn serialize_toml_scalar(v: &toml::Value) -> Option<String> {
    match v {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Integer(i) => Some(i.to_string()),
        toml::Value::Float(f) => Some(f.to_string()),
        toml::Value::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
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
