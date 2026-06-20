use serde::Serialize;
use std::collections::HashMap;

use super::message::{ChatMessage, Role, ToolCall};
use super::settings::ToolDef;

/// OpenAI-compatible request structures
#[derive(Debug, Clone, Serialize)]
pub struct OpenAIRequest<'a> {
    pub model: &'a str,
    pub messages: Vec<serde_json::Value>,
    pub stream: bool,
    pub temperature: f32,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<&'a str>,
}

/// The UI stores tool output on `assistant.toolCalls[].result` and may omit `role: tool` rows.
/// Before OpenAI-wire serialization we expand into canonical assistant + synthetic `role: tool`
/// rows (one per call id), so native tool-calling providers receive complete context.
fn expand_tool_messages_for_openai_request(msgs: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::with_capacity(msgs.len());
    let mut i = 0usize;
    while i < msgs.len() {
        let m = &msgs[i];
        if matches!(m.role, Role::Assistant) {
            if let Some(tcs) = &m.tool_calls {
                let required: Vec<&ToolCall> = tcs.iter().filter(|t| !t.id.is_empty()).collect();
                if !required.is_empty() {
                    let mut j = i + 1;
                    while j < msgs.len() && matches!(msgs[j].role, Role::Tool) {
                        j += 1;
                    }
                    let following = &msgs[(i + 1)..j];
                    let mut by_id: HashMap<String, String> = HashMap::new();
                    for tm in following {
                        if let Some(id) = &tm.tool_call_id {
                            if !id.is_empty() {
                                by_id.insert(id.clone(), tm.content.clone());
                            }
                        }
                    }

                    out.push(m.clone());
                    for tc in required {
                        if tool_registry_base_name(&tc.name) == "response" {
                            continue;
                        }
                        let content = by_id
                            .get(tc.id.as_str())
                            .cloned()
                            .unwrap_or_else(|| synthetic_tool_content_for_replay(tc));
                        out.push(ChatMessage {
                            id: format!("tool_{}", uuid::Uuid::new_v4().simple()),
                            role: Role::Tool,
                            content,
                            status: "done".into(),
                            created_at: m.created_at,
                            tool_calls: None,
                            tool_call_id: Some(tc.id.clone()),
                            error_message: None,
                            reasoning: None,
                            thoughts: None,
                            headline: None,
                            raw_content: None,
                            tool_raw_output: None,
                            agent_id: None,
                            agent_instance_id: None,
                            agent_name: None,
                            agent_trace: None,
                            image_slot_labels: None,
                            images_base64: None,
                            computer_round_screen_rel_path: None,
        ui_bindings: None,
            context_state: None,
        attachments: None,
        anchor_message_id: None,
        trace_id: None,
        task_id: None,
        spawn_depth: None,
            });
                    }
                    i = j;
                    continue;
                }
            }
        }
        if matches!(m.role, Role::Tool) {
            log::warn!(
                "expand_tool_messages_for_openai_request: skipping orphan tool message id={} (no preceding assistant with tool_calls)",
                m.id
            );
        } else {
            out.push(m.clone());
        }
        i += 1;
    }
    out
}

fn tool_registry_base_name(name: &str) -> &str {
    match name.trim().split_once(':') {
        Some((base, rest)) if !base.is_empty() && !rest.trim().is_empty() => base.trim(),
        _ => name.trim(),
    }
}

fn synthetic_tool_content_for_replay(tc: &ToolCall) -> String {
    if let Some(e) = &tc.error {
        if !e.trim().is_empty() {
            return format!("ERROR: {e}");
        }
    }
    tc.result.clone().unwrap_or_else(|| {
        "{\"warning\":\"tool output missing in stored message history\"}".to_string()
    })
}

/// System prompt slices for `stream_chat`: **cacheable** (stable per session) vs **dynamic** (per round).
#[derive(Debug, Clone, Default)]
pub struct SystemPromptSections {
    /// COMMUNICATION_PUBLIC, agent prompts, tool appendix — stable across tool rounds.
    pub cacheable: Vec<String>,
    /// Per-round slices only (e.g. `[TASK_BOARD]` from `before_main_llm_call` hooks).
    pub dynamic: Vec<String>,
}

impl SystemPromptSections {
    pub fn is_empty(&self) -> bool {
        self.cacheable.is_empty() && self.dynamic.is_empty()
    }

    pub fn slice_count(&self) -> usize {
        self.cacheable.len() + self.dynamic.len()
    }

    /// One-shot callers (`chat_once`) with no per-round dynamic tail.
    pub fn all_cacheable(parts: Vec<String>) -> Self {
        Self {
            cacheable: parts,
            dynamic: Vec::new(),
        }
    }
}

fn join_prompt_slices(slices: &[String]) -> String {
    slices
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Append HTTP `system` message(s) from [`SystemPromptSections`].
///
/// When `explicit_system_cache` is on and `cacheable` is non-empty, the cache marker sits on the
/// **cacheable** block only; `dynamic` (typically `[TASK_BOARD]` only) follows as a second content part.
fn push_openai_system_messages(
    out: &mut Vec<serde_json::Value>,
    sections: &SystemPromptSections,
    explicit_system_cache: bool,
) {
    if sections.is_empty() {
        return;
    }
    let cacheable_text = join_prompt_slices(&sections.cacheable);
    let dynamic_text = join_prompt_slices(&sections.dynamic);

    if explicit_system_cache && !cacheable_text.is_empty() {
        let mut parts = vec![serde_json::json!({
            "type": "text",
            "text": cacheable_text,
            "cache_control": { "type": "ephemeral" }
        })];
        if !dynamic_text.is_empty() {
            parts.push(serde_json::json!({
                "type": "text",
                "text": dynamic_text
            }));
        }
        out.push(serde_json::json!({
            "role": "system",
            "content": parts
        }));
        return;
    }

    let mut merged = sections.cacheable.clone();
    merged.extend(sections.dynamic.clone());
    let system_text = join_prompt_slices(&merged);
    if !system_text.is_empty() {
        out.push(serde_json::json!({
            "role": "system",
            "content": system_text
        }));
    }
}

fn append_stripped_user_images_note(m: &ChatMessage, content: &mut String) -> String {
    let Some(imgs) = m.images_base64.as_ref() else {
        return content.clone();
    };
    if imgs.is_empty() {
        return content.clone();
    }
    let labels = m.image_slot_labels.as_deref();
    for (i, _) in imgs.iter().enumerate() {
        let lab = labels
            .and_then(|labs| labs.get(i))
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.as_str())
            .unwrap_or("[image attachment]");
        if !content.is_empty() {
            content.push_str("\n\n");
        }
        content.push_str(lab);
        content.push_str(" (image not inlined: primary model does not support vision)");
    }
    content.clone()
}

pub fn make_openai_messages(
    msgs: &[ChatMessage],
    system: &SystemPromptSections,
    include_reasoning_in_api: bool,
    explicit_system_cache: bool,
    inline_vision: bool,
) -> Vec<serde_json::Value> {
    let included = crate::message_context::filter_context_messages(msgs);
    let expanded = expand_tool_messages_for_openai_request(&included);
    let mut out: Vec<serde_json::Value> = Vec::new();
    push_openai_system_messages(&mut out, system, explicit_system_cache);
    for m in &expanded {
        match m.role {
            Role::System => out.push(serde_json::json!({
                "role": "system", "content": m.content
            })),
            Role::User => {
                let api_content = crate::media::append_user_attachments_api_context(
                    &m.content,
                    m.attachments.as_deref().unwrap_or(&[]),
                );
                if let Some(ref imgs) = m.images_base64 {
                    if !imgs.is_empty() && !inline_vision {
                        log::warn!(
                            "make_openai_messages: stripping {} inline image(s); model does not support vision",
                            imgs.len()
                        );
                        let mut flat = api_content.clone();
                        flat = append_stripped_user_images_note(m, &mut flat);
                        out.push(serde_json::json!({
                            "role": "user",
                            "content": flat
                        }));
                        continue;
                    }
                    if !imgs.is_empty() {
                        let mut parts: Vec<serde_json::Value> = Vec::new();
                        if !api_content.trim().is_empty() {
                            parts.push(serde_json::json!({
                                "type": "text",
                                "text": api_content
                            }));
                        }
                        let labels = m.image_slot_labels.as_deref();
                        if let Some(labs) = labels {
                            if labs.len() != imgs.len() {
                                log::warn!(
                                    "user message image_slot_labels len {} != images_base64 len {}",
                                    labs.len(),
                                    imgs.len()
                                );
                            }
                        }
                        for (i, b64) in imgs.iter().enumerate() {
                            if let Some(lab) = labels.and_then(|labs| labs.get(i)) {
                                if !lab.trim().is_empty() {
                                    parts.push(serde_json::json!({
                                        "type": "text",
                                        "text": format!("{lab}\n")
                                    }));
                                }
                            }
                            let mime = crate::agents::computer::vision::screen::image_data_url_mime_from_base64(b64);
                            let url = format!("data:{mime};base64,{b64}");
                            parts.push(serde_json::json!({
                                "type": "image_url",
                                "image_url": { "url": url }
                            }));
                        }
                        out.push(serde_json::json!({
                            "role": "user",
                            "content": parts
                        }));
                        continue;
                    }
                }
                out.push(serde_json::json!({
                    "role": "user", "content": api_content
                }));
            }
            Role::Assistant => {
                let mut obj = serde_json::Map::new();
                obj.insert("role".into(), "assistant".into());
                obj.insert(
                    "content".into(),
                    serde_json::Value::String(m.content.clone()),
                );
                // DeepSeek 等「思考模式」在流式里下发 `reasoning_content`；下一轮请求必须原样带回，
                // 否则 400 — 可由设置 `reasoningInMessages` 关闭（关闭后勿对该类模型开思考）。
                if include_reasoning_in_api {
                    if let Some(ref r) = m.reasoning {
                        if !r.is_empty() {
                            obj.insert(
                                "reasoning_content".into(),
                                serde_json::Value::String(r.clone()),
                            );
                        }
                    }
                }
                if let Some(tcs) = &m.tool_calls {
                    let tool_calls: Vec<serde_json::Value> = tcs
                        .iter()
                        .filter(|t| {
                            !t.id.trim().is_empty()
                                && !t.name.trim().is_empty()
                                && tool_registry_base_name(&t.name) != "response"
                        })
                        .map(|t| {
                            serde_json::json!({
                                "id": t.id,
                                "type": "function",
                                "function": {
                                    "name": t.name,
                                    "arguments": t.arguments
                                }
                            })
                        })
                        .collect();
                    if !tool_calls.is_empty() {
                        obj.insert("tool_calls".into(), serde_json::Value::Array(tool_calls));
                    }
                }
                out.push(serde_json::Value::Object(obj));
            }
            Role::Tool => out.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": m.tool_call_id.clone().unwrap_or_default(),
                "content": m.content
            })),
        }
    }
    out
}

#[cfg(test)]
mod make_openai_messages_tests {
    use super::*;
    use crate::models::{ExcludedReason, MessageContextState};

    fn msg(role: Role) -> ChatMessage {
        ChatMessage {
            id: "m".into(),
            role,
            content: String::new(),
            status: "done".into(),
            created_at: 0,
            tool_calls: None,
            tool_call_id: None,
            error_message: None,
            reasoning: None,
            thoughts: None,
            headline: None,
            raw_content: None,
            tool_raw_output: None,
            agent_id: None,
            agent_instance_id: None,
            agent_name: None,
            agent_trace: None,
            image_slot_labels: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
        ui_bindings: None,
            context_state: None,
        attachments: None,
        anchor_message_id: None,
        trace_id: None,
        task_id: None,
        spawn_depth: None,
            }
    }

    #[test]
    fn system_prompt_uses_ephemeral_cache_control_on_cacheable_only() {
        let system = SystemPromptSections {
            cacheable: vec!["static system".into()],
            dynamic: vec!["task board".into()],
        };
        let out = make_openai_messages(&[], &system, false, true, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "system");
        let content = out[0]["content"].as_array().expect("multipart system");
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "static system");
        assert_eq!(content[0]["cache_control"]["type"], "ephemeral");
        assert_eq!(content[1]["text"], "task board");
        assert!(content[1].get("cache_control").is_none());
    }

    #[test]
    fn system_prompt_plain_string_when_cache_disabled() {
        let system = SystemPromptSections::all_cacheable(vec!["static system".into()]);
        let out = make_openai_messages(&[], &system, false, false, false);
        assert_eq!(out[0]["content"], "static system");
    }

    #[test]
    fn user_message_with_images_uses_multipart_content() {
        let mut u = msg(Role::User);
        u.content = "see screen".into();
        u.images_base64 = Some(vec!["iVBORw0KGgo=".into()]);
        let out = make_openai_messages(&[u], &SystemPromptSections::default(), false, false, true);
        assert_eq!(out.len(), 1);
        let content = out[0]["content"].as_array().expect("multipart content");
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image_url");
        assert!(content[1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
    }

    #[test]
    fn user_message_without_vision_flattens_images_to_text() {
        let mut u = msg(Role::User);
        u.content = "what is this".into();
        u.images_base64 = Some(vec!["iVBORw0KGgo=".into()]);
        let out = make_openai_messages(&[u], &SystemPromptSections::default(), false, false, false);
        assert_eq!(out[0]["role"], "user");
        let content = out[0]["content"].as_str().expect("text content");
        assert!(content.contains("what is this"));
        assert!(content.contains("does not support vision"));
        assert!(out[0]["content"].as_array().is_none());
    }

    #[test]
    fn user_message_interleaves_slot_label_before_each_image() {
        let mut u = msg(Role::User);
        u.content = "[CUR_SCREEN] preamble".into();
        u.image_slot_labels = Some(vec!["[Screen after action]".into()]);
        u.images_base64 = Some(vec!["iVBORw0KGgo=".into()]);
        let out = make_openai_messages(&[u], &SystemPromptSections::default(), false, false, true);
        let content = out[0]["content"].as_array().expect("multipart content");
        assert_eq!(content.len(), 3);
        assert_eq!(content[0]["text"], "[CUR_SCREEN] preamble");
        assert_eq!(content[1]["text"], "[Screen after action]\n");
        assert_eq!(content[2]["type"], "image_url");
    }

    #[test]
    fn assistant_includes_reasoning_content_when_present() {
        let mut a = msg(Role::Assistant);
        a.content = "answer".into();
        a.reasoning = Some("step 1…".into());
        let out = make_openai_messages(&[a], &SystemPromptSections::default(), true, false, false);
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[0]["content"], "answer");
        assert_eq!(out[0]["reasoning_content"], "step 1…");
    }

    #[test]
    fn assistant_omits_reasoning_content_when_disabled() {
        let mut a = msg(Role::Assistant);
        a.content = "answer".into();
        a.reasoning = Some("hidden".into());
        let out = make_openai_messages(&[a], &SystemPromptSections::default(), false, false, false);
        assert!(out[0].as_object().unwrap().get("reasoning_content").is_none());
    }

    #[test]
    fn assistant_then_user_json_per_computer_style() {
        let mut a = msg(Role::Assistant);
        a.content = "x".into();
        a.tool_calls = Some(vec![ToolCall {
            id: "call_abc".into(),
            name: "f".into(),
            arguments: "{}".into(),
            status: "pending".into(),
            result: None,
            error: None,
            duration_ms: None,
            risk_level: None,
            display_label: None,
            display_summary: None,
        }]);
        let mut t = msg(Role::Tool);
        t.tool_call_id = Some("call_abc".into());
        t.content = "{}".into();

        let out = make_openai_messages(&[a, t], &SystemPromptSections::default(), true, false, false);
        assert_eq!(out.len(), 2, "assistant + tool");
        assert_eq!(out[0]["role"], "assistant");
        assert!(out[0].as_object().unwrap().get("tool_calls").is_some());
        assert_eq!(out[0]["content"], "x");
        assert_eq!(out[1]["role"], "tool");
        assert_eq!(out[1]["tool_call_id"], "call_abc");
        assert_eq!(out[1]["content"], "{}");
    }

    #[test]
    fn synthesizes_inline_tool_as_tool_message_after_expand() {
        let mut a = msg(Role::Assistant);
        a.content = "calling".into();
        a.tool_calls = Some(vec![ToolCall {
            id: "call_inline".into(),
            name: "read".into(),
            arguments: "{}".into(),
            status: "success".into(),
            result: Some("file body".into()),
            error: None,
            duration_ms: None,
            risk_level: None,
            display_label: None,
            display_summary: None,
        }]);
        let out = make_openai_messages(&[a], &SystemPromptSections::default(), true, false, false);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[0]["content"], "calling");
        assert_eq!(out[1]["role"], "tool");
        assert_eq!(out[1]["tool_call_id"], "call_inline");
        assert_eq!(out[1]["content"], "file body");
    }

    #[test]
    fn excluded_messages_omitted_from_openai_request() {
        let mut excluded = msg(Role::User);
        excluded.content = "old turn".into();
        excluded.context_state = Some(MessageContextState {
            included: false,
            excluded_reason: Some(ExcludedReason::ContextCompression),
        });
        let mut included = msg(Role::User);
        included.content = "recent turn".into();
        let out = make_openai_messages(
            &[excluded, included],
            &SystemPromptSections::default(),
            false,
            false,
            false,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["content"], "recent turn");
    }

    #[test]
    fn assistant_prefers_raw_content_on_wire() {
        let mut a = msg(Role::Assistant);
        a.content = "visible".into();
        a.raw_content = Some("<response><tool_name>x</tool_name></response>".into());
        a.tool_calls = Some(vec![ToolCall {
            id: "c1".into(),
            name: "wait".into(),
            arguments: "{}".into(),
            status: "success".into(),
            result: Some("done".into()),
            error: None,
            duration_ms: None,
            risk_level: None,
            display_label: None,
            display_summary: None,
        }]);
        let out = make_openai_messages(&[a], &SystemPromptSections::default(), false, false, false);
        assert_eq!(out[0]["content"], "visible");
        assert!(out[0].as_object().unwrap().get("tool_calls").is_some());
    }

    #[test]
    fn response_tool_has_no_user_tool_result_message() {
        let mut a = msg(Role::Assistant);
        a.content = "".into();
        a.raw_content = Some("<response><tool_name>response</tool_name></response>".into());
        a.tool_calls = Some(vec![ToolCall {
            id: "c_resp".into(),
            name: "response".into(),
            arguments: r#"{"text":"Hi"}"#.into(),
            status: "success".into(),
            result: None,
            error: None,
            duration_ms: None,
            risk_level: None,
            display_label: None,
            display_summary: None,
        }]);

        let out = make_openai_messages(&[a], &SystemPromptSections::default(), false, false, false);
        assert_eq!(out.len(), 1, "assistant only");
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[0]["content"], "");
        assert!(out[0].as_object().unwrap().get("tool_calls").is_none());
    }

    #[test]
    fn orphan_tool_message_skipped_on_wire() {
        let mut assistant = msg(Role::Assistant);
        assistant.content = "done".into();
        assistant.tool_calls = Some(vec![ToolCall {
            id: "call_ok".into(),
            name: "terminal".into(),
            arguments: "{}".into(),
            status: "success".into(),
            result: Some("ok".into()),
            error: None,
            duration_ms: None,
            risk_level: None,
            display_label: None,
            display_summary: None,
        }]);
        let mut orphan = msg(Role::Tool);
        orphan.id = "tool_orphan".into();
        orphan.content = "stale db tail".into();
        orphan.tool_call_id = Some("call_orphan".into());

        let out = make_openai_messages(
            &[assistant.clone(), orphan],
            &SystemPromptSections::default(),
            false,
            false,
            false,
        );
        assert_eq!(out.len(), 2, "assistant + synthesized tool from inline result");
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[1]["role"], "tool");
        assert_eq!(out[1]["tool_call_id"], "call_ok");
    }

    #[test]
    fn orphan_tool_after_user_skipped_on_wire() {
        let user = msg(Role::User);
        let mut orphan = msg(Role::Tool);
        orphan.id = "tool_orphan".into();
        orphan.content = "orphan".into();
        orphan.tool_call_id = Some("call_orphan".into());

        let out = make_openai_messages(
            &[user, orphan],
            &SystemPromptSections::default(),
            false,
            false,
            false,
        );
        assert_eq!(out.len(), 1, "user only; orphan tool dropped");
        assert_eq!(out[0]["role"], "user");
    }
}

pub type ToolMap = HashMap<String, ToolDef>;

