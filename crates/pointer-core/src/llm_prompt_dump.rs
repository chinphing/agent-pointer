//! 调试：把发往 chat/completions 的 `messages` 与模型参数写入本地 JSON 文件。

use crate::models::ModelSettings;
use crate::storage;
use chrono::Local;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

const ENV_FORCE_DUMP: &str = "POINTER_DEBUG_LLM_PROMPTS";
/// When set to `1` or `true`, log the full OpenAI-compatible JSON body (redacted, truncated) at **info**,
/// without requiring file dump. Also enabled when [`should_dump`] is true.
const ENV_REQUEST_BODY_LOG: &str = "POINTER_DEBUG_OPENAI_REQUEST";

pub fn should_dump(settings: &ModelSettings) -> bool {
    if settings.debug_dump_llm_prompts {
        return true;
    }
    std::env::var_os(ENV_FORCE_DUMP)
        .map(|v| v == "1" || v.to_string_lossy().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Advanced pipeline Position/Verify debug tool cards in chat (调试模式 Bug 按钮).
/// Separate from [`should_dump`] so UI works without LLM prompt file dumps.
pub fn pipeline_debug_ui_enabled(settings: &ModelSettings) -> bool {
    if settings.debug_menus_enabled || should_dump(settings) {
        return true;
    }
    crate::platform_config::effective_settings_global().debug_menus_enabled
}

fn request_body_log_enabled(settings: &ModelSettings) -> bool {
    should_dump(settings)
        || std::env::var(ENV_REQUEST_BODY_LOG)
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
}

const OPENAI_REQUEST_LOG_MAX_CHARS: usize = 32_768;

/// Decision Advanced pipeline system slice marker (`decision/communication.md`).
const DECISION_SYSTEM_PROMPT_MARKER: &str = "Modular decision role";

fn system_prompt_char_len(content: &Value) -> usize {
    match content {
        Value::String(s) => s.chars().count(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .map(|t| t.chars().count())
            .sum(),
        _ => 0,
    }
}

fn system_prompt_has_decision_marker(content: &Value) -> bool {
    match content {
        Value::String(s) => s.contains(DECISION_SYSTEM_PROMPT_MARKER),
        Value::Array(parts) => parts.iter().any(|p| {
            p.get("text")
                .and_then(|t| t.as_str())
                .is_some_and(|t| t.contains(DECISION_SYSTEM_PROMPT_MARKER))
        }),
        _ => false,
    }
}

fn system_prompt_log_placeholder(content: &Value) -> String {
    let len = system_prompt_char_len(content);
    let part_count = match content {
        Value::Array(parts) => parts.len(),
        Value::String(_) => 1,
        _ => 0,
    };
    let label = if system_prompt_has_decision_marker(content) {
        "decision system prompt"
    } else {
        "system prompt"
    };
    if part_count > 1 {
        format!("[omitted {label}, {len} chars, {part_count} parts]")
    } else {
        format!("[omitted {label}, {len} chars]")
    }
}

/// Replace long `role: system` bodies with placeholders so request logs stay readable.
fn redact_system_prompts_for_log(v: &mut Value) {
    let Some(messages) = v.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return;
    };
    for msg in messages.iter_mut() {
        if msg.get("role").and_then(|r| r.as_str()) != Some("system") {
            continue;
        }
        let Some(content) = msg.get("content").cloned() else {
            continue;
        };
        if system_prompt_char_len(&content) == 0 {
            continue;
        }
        let placeholder = system_prompt_log_placeholder(&content);
        if let Some(obj) = msg.as_object_mut() {
            obj.insert("content".into(), Value::String(placeholder));
        }
    }
}

/// Log the JSON body sent to OpenAI-compatible `POST …/chat/completions` (images redacted; long bodies truncated).
/// Enable with [`should_dump`] / `POINTER_DEBUG_LLM_PROMPTS` or **`POINTER_DEBUG_OPENAI_REQUEST=1`**.
pub fn try_log_openai_chat_request_json<T: serde::Serialize>(
    settings: &ModelSettings,
    phase: &str,
    label: Option<&str>,
    url: &str,
    request: &T,
) {
    if !request_body_log_enabled(settings) {
        return;
    }
    let Ok(mut body) = serde_json::to_value(request) else {
        log::warn!("openai_chat_request_json: serialize failed phase={phase}");
        return;
    };
    redact_large_images(&mut body);
    redact_system_prompts_for_log(&mut body);
    let pretty = match serde_json::to_string_pretty(&body) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("openai_chat_request_json: to_string_pretty: {e} phase={phase}");
            return;
        }
    };
    let (snippet, truncated_note) = if pretty.len() > OPENAI_REQUEST_LOG_MAX_CHARS {
        let mut end = OPENAI_REQUEST_LOG_MAX_CHARS;
        while end > 0 && !pretty.is_char_boundary(end) {
            end -= 1;
        }
        (
            &pretty[..end],
            format!(
                "\n… [log truncated to {} chars; total JSON length {}]",
                end,
                pretty.len()
            ),
        )
    } else {
        (pretty.as_str(), String::new())
    };
    log::info!(
        "openai_chat_request_json phase={} label={:?} url={}{}{}",
        phase,
        label,
        url,
        truncated_note,
        if snippet.is_empty() {
            String::new()
        } else {
            format!("\n{}", snippet)
        }
    );
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

const UNSCOPED_DUMP_SEGMENT: &str = "_unscoped";

fn dump_file_uuid_suffix() -> String {
    let hex = uuid::Uuid::new_v4().simple().to_string();
    hex[hex.len().saturating_sub(16)..].to_string()
}

/// `{logs/llm_prompts}/{sanitized conversation id}` (Windows-safe; empty id → `_unscoped`).
pub(crate) fn conversation_dump_segment(conversation_id: Option<&str>) -> String {
    let Some(raw) = conversation_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return UNSCOPED_DUMP_SEGMENT.to_string();
    };
    let seg = crate::storage::sanitize_storage_dir_segment(raw);
    if seg.is_empty() {
        UNSCOPED_DUMP_SEGMENT.to_string()
    } else {
        seg
    }
}

/// 写入 `{app_data}/logs/llm_prompts/{conversation_id}/{ms}_{uuid16}.json`
/// （文件名短；uuid 仅后 16 位；`label` / `phase` / `conversationId` 在 JSON 内）。
pub fn try_dump_round(
    settings: &ModelSettings,
    conversation_id: Option<&str>,
    label: Option<&str>,
    phase: &str,
    stream: bool,
    _max_tokens: u32,
    messages: &[Value],
) {
    if !should_dump(settings) {
        return;
    }
    let Ok(root) = storage::app_data_dir() else {
        log::warn!("llm_prompt_dump: app data dir unavailable");
        return;
    };
    let conv_seg = conversation_dump_segment(conversation_id);
    let dir = root.join("logs").join("llm_prompts").join(&conv_seg);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("llm_prompt_dump: create_dir_all {}: {e}", dir.display());
        return;
    }
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let fname = format!("{}_{}.json", ms, dump_file_uuid_suffix());
    let path = dir.join(fname);

    let mut msgs: Vec<Value> = messages.to_vec();
    for m in msgs.iter_mut() {
        redact_large_images(m);
    }
    let mut messages_value = Value::Array(msgs);
    redact_system_prompts_for_log(&mut messages_value);
    let msgs = messages_value.as_array().cloned().unwrap_or_default();

    let extra_body = crate::models::effective_chat_extra_body(settings);
    let mut body = serde_json::json!({
        "dumpedAt": Local::now().to_rfc3339(),
        "conversationId": conversation_id.unwrap_or(""),
        "phase": phase,
        "label": label.unwrap_or(""),
        "labelStem": label.map(sanitize_stem).unwrap_or_default(),
        "model": settings.model,
        "stream": stream,
        "temperature": crate::models::effective_temperature(settings),
        "maxTokens": crate::models::effective_max_tokens(settings),
        "messages": msgs,
    });
    if let Some(Value::Object(extra_map)) = extra_body {
        if let Value::Object(ref mut map) = body {
            for (k, v) in extra_map {
                map.insert(k, v);
            }
        }
    }

    match serde_json::to_string_pretty(&body) {
        Ok(s) => match std::fs::write(&path, s) {
            Ok(()) => log::info!("llm_prompt_dump: wrote {}", path.display()),
            Err(e) => log::warn!("llm_prompt_dump: write {}: {e}", path.display()),
        },
        Err(e) => log::warn!("llm_prompt_dump: serialize: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redact_system_prompts_replaces_decision_slice_with_placeholder() {
        let mut body = json!({
            "messages": [
                {
                    "role": "system",
                    "content": "prefix\nModular decision role\nsuffix with lots of text"
                },
                { "role": "user", "content": "hello" }
            ]
        });
        redact_system_prompts_for_log(&mut body);
        let sys = &body["messages"][0]["content"];
        assert_eq!(
            sys.as_str().unwrap(),
            "[omitted decision system prompt, 53 chars]"
        );
        assert_eq!(body["messages"][1]["content"], "hello");
    }

    #[test]
    fn redact_system_prompts_handles_multipart_system_content() {
        let mut body = json!({
            "messages": [{
                "role": "system",
                "content": [
                    { "type": "text", "text": "Modular decision role\nblock A" },
                    { "type": "text", "text": "block B" }
                ]
            }]
        });
        redact_system_prompts_for_log(&mut body);
        assert_eq!(
            body["messages"][0]["content"].as_str().unwrap(),
            "[omitted decision system prompt, 36 chars, 2 parts]"
        );
    }

    #[test]
    fn conversation_dump_segment_sanitizes_im_ids() {
        assert_eq!(
            conversation_dump_segment(Some("wecom:default:dm:chat")),
            "wecom_default_dm_chat"
        );
        assert_eq!(conversation_dump_segment(None), "_unscoped");
        assert_eq!(conversation_dump_segment(Some("  ")), "_unscoped");
        assert_eq!(conversation_dump_segment(Some("abc-123")), "abc-123");
    }

    #[test]
    fn dump_file_uuid_suffix_is_16_hex_chars() {
        let s = dump_file_uuid_suffix();
        assert_eq!(s.len(), 16);
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
