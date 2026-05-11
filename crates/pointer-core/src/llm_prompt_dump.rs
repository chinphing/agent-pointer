//! 调试：把发往 chat/completions 的 `messages` 与模型参数写入本地 JSON 文件。

use crate::models::ModelSettings;
use crate::storage;
use chrono::Local;
use log;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

const ENV_FORCE_DUMP: &str = "POINTER_DEBUG_LLM_PROMPTS";

pub fn should_dump(settings: &ModelSettings) -> bool {
    if settings.debug_dump_llm_prompts {
        return true;
    }
    std::env::var_os(ENV_FORCE_DUMP)
        .map(|v| v == "1" || v.to_string_lossy().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn sanitize_stem(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    t.chars().take(96).collect()
}

fn redact_large_images(v: &mut Value) {
    match v {
        Value::Array(a) => {
            for x in a.iter_mut() {
                redact_large_images(x);
            }
        }
        Value::Object(o) => {
            if let Some(Value::String(s)) = o.get_mut("url") {
                if s.starts_with("data:image") && s.len() > 800 {
                    let n = s.len();
                    *s = format!("[omitted image base64, {n} chars]");
                }
            }
            for (_, x) in o.iter_mut() {
                redact_large_images(x);
            }
        }
        _ => {}
    }
}

/// 写入 `{app_data}/logs/llm_prompts/{ms}_{uuid}.json`（文件名短；`label` / `phase` 在 JSON 内）。
pub fn try_dump_round(
    settings: &ModelSettings,
    label: Option<&str>,
    phase: &str,
    stream: bool,
    max_tokens: u32,
    messages: &[Value],
) {
    if !should_dump(settings) {
        return;
    }
    let Ok(root) = storage::app_data_dir() else {
        log::warn!("llm_prompt_dump: app data dir unavailable");
        return;
    };
    let dir = root.join("logs").join("llm_prompts");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("llm_prompt_dump: create_dir_all: {e}");
        return;
    }
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let fname = format!("{}_{}.json", ms, uuid::Uuid::new_v4().simple());
    let path = dir.join(fname);

    let mut msgs: Vec<Value> = messages.to_vec();
    for m in msgs.iter_mut() {
        redact_large_images(m);
    }

    let body = serde_json::json!({
        "dumpedAt": Local::now().to_rfc3339(),
        "phase": phase,
        "label": label.unwrap_or(""),
        "labelStem": label.map(sanitize_stem).unwrap_or_default(),
        "model": settings.model,
        "stream": stream,
        "temperature": settings.temperature,
        "maxTokens": max_tokens,
        "messages": msgs,
    });

    match serde_json::to_string_pretty(&body) {
        Ok(s) => match std::fs::write(&path, s) {
            Ok(()) => log::info!("llm_prompt_dump: wrote {}", path.display()),
            Err(e) => log::warn!("llm_prompt_dump: write {}: {e}", path.display()),
        },
        Err(e) => log::warn!("llm_prompt_dump: serialize: {e}"),
    }
}
