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
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let workspace_cfg = manifest_dir.join("../../.pointer-build.toml");
    let local_cfg = manifest_dir.join(".pointer-build.toml");
    println!("cargo:rerun-if-changed={}", workspace_cfg.display());
    println!("cargo:rerun-if-changed={}", local_cfg.display());

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
