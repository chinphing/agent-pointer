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

fn build_cjk_fts_extension() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let src = manifest_dir.join("vendor/sqlite-cjk-fts/cjk_tokenizer.c");
    let include_dir = manifest_dir.join("vendor/sqlite-cjk-fts");
    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-changed={}", include_dir.join("sqlite3ext.h").display());

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let out_path = out_dir.join(cjk_fts_lib_name());

    let mut build = cc::Build::new();
    build.include(&include_dir).opt_level(2);
    let compiler = build.get_compiler();
    let compiler_path = compiler.path().to_string_lossy().into_owned();

    // cc::Tool::to_command sets MSVC INCLUDE/LIB/PATH; bare cl.exe often fails on Windows.
    let mut cmd = compiler.to_command();
    if compiler.is_like_msvc() {
        // MSVC treats `\t`, `\n`, … as escapes; Cargo OUT_DIR is under `\target\...`.
        let include = msvc_cl_path(&include_dir);
        let src_path = msvc_cl_path(&src);
        let out = msvc_cl_path(&out_path);
        cmd.current_dir(&out_dir);
        cmd.args([
            "/nologo",
            "/TC",
            "/O2",
            "/LD",
            &format!("/I{include}"),
            &src_path,
            &format!("/Fe:{out}"),
        ]);
    } else {
        if !cfg!(target_os = "windows") {
            cmd.arg("-fPIC");
        }
        cmd.args([
            "-O2",
            "-shared",
            &format!("-I{}", include_dir.display()),
            src.to_str().expect("utf8 path"),
            "-o",
            out_path.to_str().expect("utf8 path"),
        ]);
    }

    let output = cmd.output().unwrap_or_else(|e| {
        let msg = format!(
            "failed to run C compiler {compiler_path} for sqlite-cjk-fts: {e} \
             (Windows: use x64 Native Tools / Developer PowerShell for VS; \
             install Desktop C++ workload; or set CC=gcc)"
        );
        emit_build_warnings(&msg);
        panic!("{msg}");
    });
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let msg = format!(
            "sqlite-cjk-fts compile failed (compiler={compiler_path}, exit={:?})\n\
             --- cl stdout ---\n{stdout}\n--- cl stderr ---\n{stderr}",
            output.status.code()
        );
        emit_build_warnings(&msg);
        panic!("{msg}");
    }
    if !out_path.exists() {
        let msg = format!(
            "sqlite-cjk-fts missing after compile: {}",
            out_path.display()
        );
        emit_build_warnings(&msg);
        panic!("{msg}");
    }
}

/// Emit each line as cargo:warning so failures stay visible under `npm run tauri:build`.
fn emit_build_warnings(msg: &str) {
    for line in msg.lines() {
        println!("cargo:warning=pointer-core: {line}");
    }
}

/// MSVC `cl` treats `\t`, `\n`, … as escapes in unquoted paths; use forward slashes.
fn msvc_cl_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn cjk_fts_lib_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "libcjkfts.dylib"
    } else if cfg!(target_os = "windows") {
        "libcjkfts.dll"
    } else {
        "libcjkfts.so"
    }
}
