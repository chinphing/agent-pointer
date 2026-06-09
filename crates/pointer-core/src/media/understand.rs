use crate::media::token::{record_media_understand_usage, MediaTokenContext, MediaUnderstandKind};
use crate::models::{AgentModelRef, ChatMessage, ModelSettings, Role};
use crate::provider::OpenAIProvider;
use anyhow::{Context, Result};
use serde_json::json;
use tokio_util::sync::CancellationToken;

const DESCRIBE_PROMPT: &str = "Describe this image concisely for an assistant that cannot see it. \
Focus on visible text, objects, layout, and anything relevant to a user question.";

fn resolve_provider_api_key(settings: &ModelSettings, fallback_api_key: &str) -> String {
    let pid = settings.active_provider_id.trim();
    if let Some(p) = settings.providers.iter().find(|p| p.id == pid) {
        if !p.api_key.trim().is_empty() {
            return p.api_key.clone();
        }
        return String::new();
    }
    fallback_api_key.trim().to_string()
}

pub async fn describe_image_with_model(
    settings: &ModelSettings,
    image_model: &AgentModelRef,
    api_key_fallback: &str,
    image_base64: &str,
    mime_type: &str,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    let mut image_settings = settings.clone();
    if !image_model.provider_id.trim().is_empty() {
        image_settings.active_provider_id = image_model.provider_id.trim().to_string();
    }
    if !image_model.model.trim().is_empty() {
        image_settings.model = image_model.model.trim().to_string();
    }
    let api_key = resolve_provider_api_key(&image_settings, api_key_fallback);
    if api_key.is_empty() {
        anyhow::bail!("no API key for image understanding model");
    }
    let provider = OpenAIProvider::new(image_settings, api_key);
    let user = ChatMessage {
        id: "media-describe".into(),
        role: Role::User,
        content: "Describe the attached image.".into(),
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
        images_base64: Some(vec![image_base64.to_string()]),
        image_slot_labels: None,
        computer_round_screen_rel_path: None,
        ui_bindings: None,
        context_state: None,
        attachments: None,
    };
    let system = crate::models::SystemPromptSections::all_cacheable(vec![
        DESCRIBE_PROMPT.to_string(),
    ]);
    let out = provider
        .chat_once(
            &[user],
            &system,
            vec![],
            cancel.clone(),
            Some(1024),
            Some("media_image_understand"),
        )
        .await
        .context("image understanding chat_once")?;
    record_media_understand_usage(token_ctx, MediaUnderstandKind::Image, &out);
    let text = out.text.trim().to_string();
    if text.is_empty() {
        anyhow::bail!("image understanding returned empty content");
    }
    let _ = mime_type;
    Ok(text)
}

const TRANSCRIBE_PROMPT: &str = "Transcribe the attached audio to plain text. \
Reply with the transcript only, no preamble.";

fn audio_wire_format(mime_type: &str, file_name: &str) -> &'static str {
    let m = mime_type.trim().to_ascii_lowercase();
    if m.contains("wav") {
        return "wav";
    }
    if m.contains("mpeg") || m.contains("mp3") {
        return "mp3";
    }
    if m.contains("mp4") || m.contains("m4a") {
        return "mp4";
    }
    if m.contains("webm") {
        return "webm";
    }
    if m.contains("ogg") {
        return "ogg";
    }
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".wav") {
        return "wav";
    }
    if lower.ends_with(".mp3") {
        return "mp3";
    }
    if lower.ends_with(".m4a") {
        return "mp4";
    }
    if lower.ends_with(".webm") {
        return "webm";
    }
    if lower.ends_with(".ogg") {
        return "ogg";
    }
    "mp3"
}

pub async fn transcribe_audio_with_model(
    settings: &ModelSettings,
    audio_model: &AgentModelRef,
    api_key_fallback: &str,
    audio_base64: &str,
    mime_type: &str,
    file_name: &str,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    let mut audio_settings = settings.clone();
    if !audio_model.provider_id.trim().is_empty() {
        audio_settings.active_provider_id = audio_model.provider_id.trim().to_string();
    }
    if !audio_model.model.trim().is_empty() {
        audio_settings.model = audio_model.model.trim().to_string();
    }
    let api_key = resolve_provider_api_key(&audio_settings, api_key_fallback);
    if api_key.is_empty() {
        anyhow::bail!("no API key for audio understanding model");
    }
    let provider = OpenAIProvider::new(audio_settings, api_key);
    let format = audio_wire_format(mime_type, file_name);
    let messages = vec![
        json!({
            "role": "system",
            "content": TRANSCRIBE_PROMPT
        }),
        json!({
            "role": "user",
            "content": [
                {
                    "type": "input_audio",
                    "input_audio": {
                        "data": audio_base64,
                        "format": format
                    }
                },
                {
                    "type": "text",
                    "text": "Transcribe this audio."
                }
            ]
        }),
    ];
    let out = provider
        .chat_once_wire_messages(
            messages,
            cancel.clone(),
            Some(2048),
            Some("media_audio_transcribe"),
        )
        .await
        .context("audio transcription chat_once_wire")?;
    record_media_understand_usage(token_ctx, MediaUnderstandKind::Audio, &out);
    let text = out.text.trim().to_string();
    if text.is_empty() {
        anyhow::bail!("audio transcription returned empty content");
    }
    Ok(text)
}

const PDF_OCR_PROMPT: &str = "Extract and summarize the content of this PDF from the attached page images. \
Transcribe visible text accurately, preserve headings/lists/tables where possible, \
and describe non-text visuals briefly. Be concise but complete.";

const VIDEO_DESCRIBE_PROMPT: &str = "Summarize this video from the attached still frames. \
Describe the scene, actions, visible text, and anything relevant to a user question. \
Be concise but complete.";

pub async fn describe_pdf_pages_with_model(
    settings: &ModelSettings,
    image_model: &AgentModelRef,
    api_key_fallback: &str,
    page_base64s: &[String],
    file_name: &str,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    if page_base64s.is_empty() {
        anyhow::bail!("no pdf page images to describe");
    }
    let mut image_settings = settings.clone();
    if !image_model.provider_id.trim().is_empty() {
        image_settings.active_provider_id = image_model.provider_id.trim().to_string();
    }
    if !image_model.model.trim().is_empty() {
        image_settings.model = image_model.model.trim().to_string();
    }
    let api_key = resolve_provider_api_key(&image_settings, api_key_fallback);
    if api_key.is_empty() {
        anyhow::bail!("no API key for pdf image understanding model");
    }
    let provider = OpenAIProvider::new(image_settings, api_key);
    let user = ChatMessage {
        id: "media-pdf-describe".into(),
        role: Role::User,
        content: format!(
            "Extract content from the scanned PDF \"{file_name}\" using {} page image(s).",
            page_base64s.len()
        ),
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
        images_base64: Some(page_base64s.to_vec()),
        image_slot_labels: None,
        computer_round_screen_rel_path: None,
        ui_bindings: None,
        context_state: None,
        attachments: None,
    };
    let system = crate::models::SystemPromptSections::all_cacheable(vec![
        PDF_OCR_PROMPT.to_string(),
    ]);
    let out = provider
        .chat_once(
            &[user],
            &system,
            vec![],
            cancel.clone(),
            Some(4096),
            Some("media_pdf_understand"),
        )
        .await
        .context("pdf image understanding chat_once")?;
    record_media_understand_usage(token_ctx, MediaUnderstandKind::Pdf, &out);
    let text = out.text.trim().to_string();
    if text.is_empty() {
        anyhow::bail!("pdf image understanding returned empty content");
    }
    Ok(text)
}

pub async fn describe_video_with_model(
    settings: &ModelSettings,
    video_model: &AgentModelRef,
    api_key_fallback: &str,
    frame_base64s: &[String],
    file_name: &str,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    if frame_base64s.is_empty() {
        anyhow::bail!("no video frames to describe");
    }
    let mut video_settings = settings.clone();
    if !video_model.provider_id.trim().is_empty() {
        video_settings.active_provider_id = video_model.provider_id.trim().to_string();
    }
    if !video_model.model.trim().is_empty() {
        video_settings.model = video_model.model.trim().to_string();
    }
    let api_key = resolve_provider_api_key(&video_settings, api_key_fallback);
    if api_key.is_empty() {
        anyhow::bail!("no API key for video understanding model");
    }
    let provider = OpenAIProvider::new(video_settings, api_key);
    let user = ChatMessage {
        id: "media-video-describe".into(),
        role: Role::User,
        content: format!(
            "Summarize the video \"{file_name}\" from {} frame(s).",
            frame_base64s.len()
        ),
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
        images_base64: Some(frame_base64s.to_vec()),
        image_slot_labels: None,
        computer_round_screen_rel_path: None,
        ui_bindings: None,
        context_state: None,
        attachments: None,
    };
    let system = crate::models::SystemPromptSections::all_cacheable(vec![
        VIDEO_DESCRIBE_PROMPT.to_string(),
    ]);
    let out = provider
        .chat_once(
            &[user],
            &system,
            vec![],
            cancel.clone(),
            Some(2048),
            Some("media_video_understand"),
        )
        .await
        .context("video understanding chat_once")?;
    record_media_understand_usage(token_ctx, MediaUnderstandKind::Video, &out);
    let text = out.text.trim().to_string();
    if text.is_empty() {
        anyhow::bail!("video understanding returned empty content");
    }
    Ok(text)
}
