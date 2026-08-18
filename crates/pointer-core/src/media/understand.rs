use crate::media::token::{record_media_understand_usage, MediaTokenContext, MediaUnderstandKind};
use crate::models::{
    provider_uses_dashscope_compatible_api, AgentModelRef, ChatMessage, ModelSettings, Role,
};
use crate::provider::OpenAIProvider;
use anyhow::{Context, Result};
use serde_json::json;
use tokio_util::sync::CancellationToken;

const DESCRIBE_PROMPT: &str = "Describe this image for an assistant that cannot see it. \
Focus on visible text, objects, layout, and details relevant to the user's stated goal.";

const MULTI_IMAGE_DESCRIBE_PROMPT: &str =
    "Describe the attached images for an assistant that cannot see them. \
Each image may have a label with its file name. \
Follow the user's stated goal; compare or summarize across images when relevant.";

fn user_content_with_goal(base: &str, goal: &str) -> String {
    format!("{base}\n\nUser analysis goal:\n{goal}")
}

fn prepare_media_model_settings(
    settings: &ModelSettings,
    model: &AgentModelRef,
    kind: &str,
) -> ModelSettings {
    let mut s = settings.clone();
    if !model.provider_id.trim().is_empty() {
        s.active_provider_id = model.provider_id.trim().to_string();
    }
    if !model.model.trim().is_empty() {
        s.model = model.model.trim().to_string();
    }
    if let Some(cfg) = crate::mode_llm::resolve_media_mode_llm_config(settings, kind) {
        crate::thinking_strategy::apply_tier_thinking_to_round(&mut s, cfg);
    }
    s
}

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
    goal: &str,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    let image_settings = prepare_media_model_settings(settings, image_model, "image");
    let api_key = resolve_provider_api_key(&image_settings, api_key_fallback);
    if api_key.is_empty() {
        anyhow::bail!("no API key for image understanding model");
    }
    let provider = OpenAIProvider::new(image_settings, api_key);
    let user = ChatMessage {
        id: "media-describe".into(),
        role: Role::User,
        content: user_content_with_goal("Describe the attached image.", goal).into(),
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
        anchor_message_id: None,
        trace_id: None,
        task_id: None,
        spawn_depth: None,
    };
    let system =
        crate::models::SystemPromptSections::all_cacheable(vec![DESCRIBE_PROMPT.to_string()]);
    let out = provider
        .chat_once(
            &[user],
            &system,
            vec![],
            cancel.clone(),
            Some(4096),
            Some("media_image_understand"),
        )
        .await
        .context("image understanding chat_once")?;
    record_media_understand_usage(token_ctx, MediaUnderstandKind::Image, &out);
    let text = out.text.trim().to_string();
    if text.is_empty() {
        if out.finish_reason.as_deref() == Some("length") {
            anyhow::bail!(
                "image understanding output truncated at max_tokens; retry with a shorter analysis goal"
            );
        }
        anyhow::bail!("image understanding returned empty content");
    }
    let _ = mime_type;
    Ok(text)
}

pub async fn describe_images_with_model(
    settings: &ModelSettings,
    image_model: &AgentModelRef,
    api_key_fallback: &str,
    images_base64: &[String],
    labels: &[String],
    source_description: &str,
    goal: &str,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    if images_base64.is_empty() {
        anyhow::bail!("no images to describe");
    }
    let image_settings = prepare_media_model_settings(settings, image_model, "image");
    let api_key = resolve_provider_api_key(&image_settings, api_key_fallback);
    if api_key.is_empty() {
        anyhow::bail!("no API key for image understanding model");
    }
    let provider = OpenAIProvider::new(image_settings, api_key);
    let slot_labels = if labels.len() == images_base64.len() {
        Some(labels.to_vec())
    } else {
        log::warn!(
            "image dir labels len {} != images len {}",
            labels.len(),
            images_base64.len()
        );
        None
    };
    let user = ChatMessage {
        id: "media-images-describe".into(),
        role: Role::User,
        content: user_content_with_goal(
            &format!(
                "Describe {} image(s) {source_description}.",
                images_base64.len()
            ),
            goal,
        )
        .into(),
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
        images_base64: Some(images_base64.to_vec()),
        image_slot_labels: slot_labels,
        computer_round_screen_rel_path: None,
        ui_bindings: None,
        context_state: None,
        attachments: None,
        anchor_message_id: None,
        trace_id: None,
        task_id: None,
        spawn_depth: None,
    };
    let system = crate::models::SystemPromptSections::all_cacheable(vec![
        MULTI_IMAGE_DESCRIBE_PROMPT.to_string(),
    ]);
    let out = provider
        .chat_once(
            &[user],
            &system,
            vec![],
            cancel.clone(),
            Some(4096),
            Some("media_image_dir_understand"),
        )
        .await
        .context("image directory understanding chat_once")?;
    record_media_understand_usage(token_ctx, MediaUnderstandKind::Image, &out);
    let text = out.text.trim().to_string();
    if text.is_empty() {
        anyhow::bail!("image directory understanding returned empty content");
    }
    Ok(text)
}

const TRANSCRIBE_PROMPT: &str = "Transcribe the attached audio to plain text. \
Follow the user's stated goal (verbatim transcript vs summary vs key quotes). \
Reply with the result only, no preamble.";

pub async fn transcribe_audio_with_model(
    settings: &ModelSettings,
    audio_model: &AgentModelRef,
    api_key_fallback: &str,
    audio_base64: &str,
    mime_type: &str,
    file_name: &str,
    goal: &str,
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
    let format = crate::media::audio::audio_wire_format(mime_type, file_name);
    let provider_cfg = audio_settings
        .providers
        .iter()
        .find(|p| p.id == audio_settings.active_provider_id);

    if let Some(p) = provider_cfg {
        if provider_uses_dashscope_compatible_api(p) {
            let audio_url =
                crate::media::audio::wire_audio_data_for_multimodal(format, audio_base64);
            let out = crate::media::dashscope_audio::transcribe_audio_dashscope_multimodal(
                p,
                &api_key,
                &audio_settings.model,
                &audio_url,
                cancel,
            )
            .await
            .context("dashscope multimodal audio transcribe")?;
            record_media_understand_usage(token_ctx, MediaUnderstandKind::Audio, &out);
            let text = out.text.trim().to_string();
            if text.is_empty() {
                anyhow::bail!("audio transcription returned empty content");
            }
            return Ok(text);
        }
    }

    let provider = OpenAIProvider::new(audio_settings.clone(), api_key);
    let wired_audio = provider_cfg
        .map(|p| crate::media::audio::wire_audio_data_for_provider(p, format, audio_base64))
        .unwrap_or_else(|| audio_base64.to_string());
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
                        "data": wired_audio,
                        "format": format
                    }
                },
                {
                    "type": "text",
                    "text": user_content_with_goal("Transcribe this audio.", goal)
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

const PDF_OCR_PROMPT: &str = "Extract and summarize PDF content from the attached page images. \
Follow the user's stated goal for what to include, emphasize, or omit. \
Transcribe visible text accurately; preserve headings/lists/tables where relevant.";

const VIDEO_DESCRIBE_PROMPT: &str = "Summarize this video from the attached still frames. \
Follow the user's stated goal for scene, actions, visible text, and key details.";

pub async fn describe_pdf_pages_with_model(
    settings: &ModelSettings,
    image_model: &AgentModelRef,
    api_key_fallback: &str,
    page_base64s: &[String],
    file_name: &str,
    goal: &str,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    if page_base64s.is_empty() {
        anyhow::bail!("no pdf page images to describe");
    }
    let image_settings = prepare_media_model_settings(settings, image_model, "image");
    let api_key = resolve_provider_api_key(&image_settings, api_key_fallback);
    if api_key.is_empty() {
        anyhow::bail!("no API key for pdf image understanding model");
    }
    let provider = OpenAIProvider::new(image_settings, api_key);
    let user = ChatMessage {
        id: "media-pdf-describe".into(),
        role: Role::User,
        content: user_content_with_goal(
            &format!(
                "Extract content from the scanned PDF \"{file_name}\" using {} page image(s).",
                page_base64s.len()
            ),
            goal,
        )
        .into(),
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
        anchor_message_id: None,
        trace_id: None,
        task_id: None,
        spawn_depth: None,
    };
    let system =
        crate::models::SystemPromptSections::all_cacheable(vec![PDF_OCR_PROMPT.to_string()]);
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
    goal: &str,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    if frame_base64s.is_empty() {
        anyhow::bail!("no video frames to describe");
    }
    let video_settings = prepare_media_model_settings(settings, video_model, "video");
    let api_key = resolve_provider_api_key(&video_settings, api_key_fallback);
    if api_key.is_empty() {
        anyhow::bail!("no API key for video understanding model");
    }
    let provider = OpenAIProvider::new(video_settings, api_key);
    let user = ChatMessage {
        id: "media-video-describe".into(),
        role: Role::User,
        content: user_content_with_goal(
            &format!(
                "Summarize the video \"{file_name}\" from {} frame(s).",
                frame_base64s.len()
            ),
            goal,
        )
        .into(),
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
        anchor_message_id: None,
        trace_id: None,
        task_id: None,
        spawn_depth: None,
    };
    let system =
        crate::models::SystemPromptSections::all_cacheable(vec![VIDEO_DESCRIBE_PROMPT.to_string()]);
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
