use crate::media::token::{record_media_understand_usage, MediaTokenContext, MediaUnderstandKind};
use crate::models::{
    provider_uses_dashscope_compatible_api, AgentModelRef, ChatMessage, ModelSettings, Role,
};
use crate::provider::{ChatOnceOutput, OpenAIProvider};
use anyhow::{Context, Result};
use serde_json::json;
use tokio_util::sync::CancellationToken;

/// Visible-answer budget reserved after thinking tokens.
///
/// DeepSeek-style APIs count `reasoning_content` toward `max_tokens`. If this
/// equals the thinking budget, long OCR / multi-image calls finish with empty
/// `content` and the tool reports a blank failure.
const MEDIA_ANSWER_MAX_TOKENS: u32 = 16384;

/// Hard ceiling for the retry ladder. Doubling the budget is how a truncated attempt
/// gets room to finish, but without a stop the last retry would ask for 160k+ tokens
/// (≈7h at the observed 6.3 tok/s). A limit the model declares still wins when smaller.
const MEDIA_UNDERSTAND_MAX_TOKENS_CEILING: u32 = 65536;

/// One first attempt plus up to two retries, each doubling the token budget.
const MEDIA_UNDERSTAND_MAX_ATTEMPTS: usize = 3;

/// Appended to the prompt when the previous attempt was cut off by the output limit and
/// therefore produced no answer: the budget went into reasoning, so the retry has to be
/// told to reason less and answer directly. Deliberately *not* the previous text — that
/// attempt returned none, and feeding reasoning back would grow the prompt instead.
const TRUNCATED_RETRY_NOTE: &str =
    "Note: the previous attempt was cut off by the output length limit and returned no \
answer. Reason much more briefly this time, then answer directly with the result.";

fn media_understand_max_tokens(settings: &ModelSettings) -> u32 {
    let thinking = settings.round_thinking_budget.unwrap_or(0);
    let wanted = thinking
        .saturating_add(MEDIA_ANSWER_MAX_TOKENS)
        .max(MEDIA_ANSWER_MAX_TOKENS);
    // Only a cap the provider/model itself declares is a real API limit. The user's
    // own output setting must not squeeze this budget: `reasoning_content` counts
    // toward `max_tokens`, so a small chat cap would leave the visible answer with
    // nothing and the tool would report a blank failure.
    match crate::models::declared_max_tokens(settings) {
        Some(cap) => wanted.min(cap),
        None => wanted,
    }
}

/// Attempt `attempt`'s budget (1-based): the first attempt uses `base` as-is and every
/// retry doubles it. The result is clamped by the hard ceiling **and** by whatever the
/// model declares — the smaller of the two wins, so doubling can never ask for more than
/// either allows.
fn attempt_budget(base: u32, attempt: usize, cap: Option<u32>) -> u32 {
    let mut wanted = base;
    for _ in 1..attempt {
        wanted = wanted.saturating_mul(2);
    }
    let wanted = wanted.min(MEDIA_UNDERSTAND_MAX_TOKENS_CEILING);
    match cap {
        Some(cap) => wanted.min(cap),
        None => wanted,
    }
}

/// True when the model ran into the output ceiling rather than stopping on its own.
fn is_length_truncated(out: &ChatOnceOutput, max_tokens: u32) -> bool {
    if out.finish_reason.as_deref() == Some("length") {
        return true;
    }
    let completion = out.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0);
    max_tokens > 0 && completion >= max_tokens
}

fn require_understand_text(kind: &str, out: &ChatOnceOutput, max_tokens: u32) -> Result<String> {
    let text = out.text.trim().to_string();
    if !text.is_empty() {
        return Ok(text);
    }
    let finish = out.finish_reason.as_deref().unwrap_or("unknown");
    let completion = out.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0);
    let reasoning_tokens = out.usage.as_ref().map(|u| u.reasoning_tokens).unwrap_or(0);
    let reasoning_chars = out
        .reasoning_content
        .as_deref()
        .map(str::chars)
        .map(Iterator::count)
        .unwrap_or(0);
    log::warn!(
        "{kind} empty content model={} finish_reason={finish} completion_tokens={completion} reasoning_tokens={reasoning_tokens} reasoning_chars={reasoning_chars} max_tokens={max_tokens}",
        out.model
    );
    if is_length_truncated(out, max_tokens) {
        anyhow::bail!(
            "{kind} output truncated at max_tokens ({completion}/{max_tokens}); retry with fewer images or a shorter analysis goal"
        );
    }
    anyhow::bail!("{kind} returned empty content (finish_reason={finish})")
}

/// One media-understanding request: what the image / image-dir / pdf / video paths differ
/// in. Everything else — budget escalation, retry, token accounting — is shared.
struct MediaUnderstandPrompt<'a> {
    /// Log / error label, e.g. `image directory understanding`.
    kind: &'a str,
    /// `chat_once` call name used for provider telemetry.
    call_name: &'a str,
    system_prompt: &'a str,
    message_id: &'a str,
    /// Task sentence without the goal, e.g. `Describe the attached image.`
    base: String,
    goal: &'a str,
    images: &'a [String],
    labels: Option<&'a [String]>,
    token_kind: MediaUnderstandKind,
}

impl MediaUnderstandPrompt<'_> {
    /// `note` carries the previous attempt's truncation notice on a retry.
    fn user_message(&self, note: Option<&str>) -> ChatMessage {
        ChatMessage {
            turn_elapsed_ms: None,
            id: self.message_id.into(),
            role: Role::User,
            content: user_content_with_retry_note(&self.base, self.goal, note).into(),
            status: "done".into(),
            created_at: 0,
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
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
            images_base64: Some(self.images.to_vec()),
            image_slot_labels: self.labels.map(|labels| labels.to_vec()),
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
            attachments: None,
            anchor_message_id: None,
            trace_id: None,
            task_id: None,
            spawn_depth: None,
            agent_chain: None,
        }
    }
}

/// Shared retry loop for every media-understanding path.
///
/// A single shot used to burn the whole budget on reasoning and come back empty, which
/// surfaced as a 30-minute call that reported nothing. Each retry doubles the budget and,
/// after a length cut-off, tells the model why the previous attempt failed.
///
/// Only `require_understand_text` failures are retried: a transport error from
/// `chat_once` propagates immediately (retrying it would multiply a broken call).
async fn understand_with_retries(
    prompt: MediaUnderstandPrompt<'_>,
    provider: &OpenAIProvider,
    settings: &ModelSettings,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    let base_tokens = media_understand_max_tokens(settings);
    let cap = crate::models::declared_max_tokens(settings);
    log::info!(
        "media understand {} model={} attempts={MEDIA_UNDERSTAND_MAX_ATTEMPTS} base_max_tokens={base_tokens} ceiling={MEDIA_UNDERSTAND_MAX_TOKENS_CEILING} declared_cap={cap:?} thinking_budget={:?}",
        prompt.kind,
        settings.model,
        settings.round_thinking_budget
    );
    let system =
        crate::models::SystemPromptSections::all_cacheable(vec![prompt.system_prompt.to_string()]);
    let mut note: Option<&str> = None;
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=MEDIA_UNDERSTAND_MAX_ATTEMPTS {
        if cancel.is_cancelled() {
            log::info!(
                "media understand {} retry skipped (cancelled) attempt={attempt}",
                prompt.kind
            );
            break;
        }
        let max_tokens = attempt_budget(base_tokens, attempt, cap);
        log::info!(
            "media understand {} attempt={attempt} max_tokens={max_tokens} base_max_tokens={base_tokens}",
            prompt.kind
        );
        let user = prompt.user_message(note);
        let out = provider
            .chat_once(
                &[user],
                &system,
                vec![],
                cancel.clone(),
                Some(max_tokens),
                Some(prompt.call_name),
            )
            .await
            .with_context(|| format!("{} chat_once", prompt.kind))?;
        // Every attempt spends tokens, including the ones we end up retrying.
        record_media_understand_usage(token_ctx, prompt.token_kind, &out);
        match require_understand_text(prompt.kind, &out, max_tokens) {
            Ok(text) => return Ok(text),
            Err(err) => {
                // Only a length cut-off is worth telling the model about; an empty
                // answer below the cap is a different failure with the same retry.
                note = is_length_truncated(&out, max_tokens).then_some(TRUNCATED_RETRY_NOTE);
                last_err = Some(err);
            }
        }
    }
    match last_err {
        Some(err) => Err(err),
        None => Err(anyhow::anyhow!("{} cancelled", prompt.kind)),
    }
}

const DESCRIBE_PROMPT: &str = "Describe this image for an assistant that cannot see it. \
Focus on visible text, objects, layout, and details relevant to the user's stated goal.";

const MULTI_IMAGE_DESCRIBE_PROMPT: &str =
    "Describe the attached images for an assistant that cannot see them. \
Each image may have a label with its file name. \
Follow the user's stated goal; compare or summarize across images when relevant.";

fn user_content_with_goal(base: &str, goal: &str) -> String {
    user_content_with_retry_note(base, goal, None)
}

/// `note` is the previous attempt's truncation notice on a retry — appended after the
/// goal so the model reads the task first and the correction last.
fn user_content_with_retry_note(base: &str, goal: &str, note: Option<&str>) -> String {
    match note.map(str::trim).filter(|note| !note.is_empty()) {
        Some(note) => format!("{base}\n\nUser analysis goal:\n{goal}\n\n{note}"),
        None => format!("{base}\n\nUser analysis goal:\n{goal}"),
    }
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
    let provider = OpenAIProvider::new(image_settings.clone(), api_key);
    let images = vec![image_base64.to_string()];
    let _ = mime_type;
    understand_with_retries(
        MediaUnderstandPrompt {
            kind: "image understanding",
            call_name: "media_image_understand",
            system_prompt: DESCRIBE_PROMPT,
            message_id: "media-describe",
            base: "Describe the attached image.".into(),
            goal,
            images: &images,
            labels: None,
            token_kind: MediaUnderstandKind::Image,
        },
        &provider,
        &image_settings,
        token_ctx,
        cancel,
    )
    .await
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
    let provider = OpenAIProvider::new(image_settings.clone(), api_key);
    let slot_labels = if labels.len() == images_base64.len() {
        Some(labels)
    } else {
        log::warn!(
            "image dir labels len {} != images len {}",
            labels.len(),
            images_base64.len()
        );
        None
    };
    log::info!(
        "media understand image_dir count={} model={}",
        images_base64.len(),
        image_settings.model
    );
    understand_with_retries(
        MediaUnderstandPrompt {
            kind: "image directory understanding",
            call_name: "media_image_dir_understand",
            system_prompt: MULTI_IMAGE_DESCRIBE_PROMPT,
            message_id: "media-images-describe",
            base: format!(
                "Describe {} image(s) {source_description}.",
                images_base64.len()
            ),
            goal,
            images: images_base64,
            labels: slot_labels,
            token_kind: MediaUnderstandKind::Image,
        },
        &provider,
        &image_settings,
        token_ctx,
        cancel,
    )
    .await
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
    let provider = OpenAIProvider::new(image_settings.clone(), api_key);
    log::info!(
        "media understand pdf pages={} model={}",
        page_base64s.len(),
        image_settings.model
    );
    understand_with_retries(
        MediaUnderstandPrompt {
            kind: "pdf image understanding",
            call_name: "media_pdf_understand",
            system_prompt: PDF_OCR_PROMPT,
            message_id: "media-pdf-describe",
            base: format!(
                "Extract content from the scanned PDF \"{file_name}\" using {} page image(s).",
                page_base64s.len()
            ),
            goal,
            images: page_base64s,
            labels: None,
            token_kind: MediaUnderstandKind::Pdf,
        },
        &provider,
        &image_settings,
        token_ctx,
        cancel,
    )
    .await
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
    let provider = OpenAIProvider::new(video_settings.clone(), api_key);
    log::info!(
        "media understand video frames={} model={}",
        frame_base64s.len(),
        video_settings.model
    );
    understand_with_retries(
        MediaUnderstandPrompt {
            kind: "video understanding",
            call_name: "media_video_understand",
            system_prompt: VIDEO_DESCRIBE_PROMPT,
            message_id: "media-video-describe",
            base: format!(
                "Summarize the video \"{file_name}\" from {} frame(s).",
                frame_base64s.len()
            ),
            goal,
            images: frame_base64s,
            labels: None,
            token_kind: MediaUnderstandKind::Video,
        },
        &provider,
        &video_settings,
        token_ctx,
        cancel,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm_token_stats::LlmUsageSnapshot;
    use crate::provider::ChatOnceOutput;

    fn empty_out(finish: Option<&str>, completion: u32) -> ChatOnceOutput {
        ChatOnceOutput {
            text: String::new(),
            usage: Some(LlmUsageSnapshot {
                prompt_tokens: 1800,
                completion_tokens: completion,
                total_tokens: 1800 + completion,
                reasoning_tokens: 0,
                cached_tokens: 0,
            }),
            model: "deepseek-flash".into(),
            tool_calls: vec![],
            reasoning_content: None,
            finish_reason: finish.map(|s| s.to_string()),
        }
    }

    /// A realistic output cap, so these assertions exercise the budget arithmetic
    /// rather than a clamp.
    fn settings_with_large_output_cap() -> ModelSettings {
        let mut s = ModelSettings::default();
        s.max_tokens = MEDIA_ANSWER_MAX_TOKENS.saturating_mul(2);
        s
    }

    #[test]
    fn max_tokens_covers_thinking_budget() {
        let mut s = settings_with_large_output_cap();
        s.round_thinking_budget = Some(4096);
        assert_eq!(
            media_understand_max_tokens(&s),
            4096 + MEDIA_ANSWER_MAX_TOKENS
        );
    }

    #[test]
    fn max_tokens_without_thinking_uses_answer_budget() {
        let s = settings_with_large_output_cap();
        assert_eq!(media_understand_max_tokens(&s), MEDIA_ANSWER_MAX_TOKENS);
    }

    /// Regression: the chat's own output setting is not an API limit. A small cap
    /// used to clamp this budget down to the thinking budget, leaving the visible
    /// answer with nothing.
    #[test]
    fn small_chat_output_cap_does_not_squeeze_media_budget() {
        let mut s = ModelSettings::default(); // max_tokens defaults to the product's 16384
        s.round_thinking_budget = Some(4096);
        assert_eq!(
            media_understand_max_tokens(&s),
            4096 + MEDIA_ANSWER_MAX_TOKENS
        );
    }

    #[test]
    fn empty_content_at_cap_is_truncation() {
        let err = require_understand_text(
            "image directory understanding",
            &empty_out(Some("length"), 4096),
            4096,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("truncated"), "{msg}");
        assert!(msg.contains("fewer images"), "{msg}");
    }

    #[test]
    fn empty_content_hitting_max_tokens_without_length_reason_is_truncation() {
        let err = require_understand_text(
            "image directory understanding",
            &empty_out(Some("stop"), 4096),
            4096,
        )
        .unwrap_err();
        assert!(err.to_string().contains("truncated"));
    }

    #[test]
    fn empty_content_below_cap_keeps_empty_error() {
        let err = require_understand_text(
            "image directory understanding",
            &empty_out(Some("stop"), 12),
            12288,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("empty content"), "{msg}");
        assert!(!msg.contains("truncated"), "{msg}");
    }

    /// The visible-answer budget the user asked for: 16k on top of the thinking budget.
    #[test]
    fn media_answer_budget_is_16k() {
        let mut s = settings_with_large_output_cap();
        s.round_thinking_budget = Some(4096);
        assert_eq!(MEDIA_ANSWER_MAX_TOKENS, 16384);
        assert_eq!(media_understand_max_tokens(&s), 20480);
    }

    /// The ladder the user asked for: 20480 → 40960 → 65536 (4096 thinking + 16384 base).
    #[test]
    fn retry_budget_doubles_each_attempt() {
        assert_eq!(MEDIA_UNDERSTAND_MAX_ATTEMPTS, 3);
        assert_eq!(attempt_budget(20480, 1, None), 20480);
        assert_eq!(attempt_budget(20480, 2, None), 40960);
        // A third attempt would double to 81920; the hard ceiling stops it at 64k.
        assert_eq!(attempt_budget(20480, 3, None), 65536);
    }

    /// The 64k ceiling is a hard stop for the ladder, whatever the base is.
    #[test]
    fn retry_budget_never_exceeds_the_ceiling() {
        assert_eq!(MEDIA_UNDERSTAND_MAX_TOKENS_CEILING, 65536);
        assert_eq!(
            attempt_budget(20480, 3, None),
            MEDIA_UNDERSTAND_MAX_TOKENS_CEILING
        );
        assert_eq!(
            attempt_budget(40000, 2, None),
            MEDIA_UNDERSTAND_MAX_TOKENS_CEILING
        );
        // Even a first attempt above the ceiling is pulled back to it.
        assert_eq!(
            attempt_budget(70000, 1, None),
            MEDIA_UNDERSTAND_MAX_TOKENS_CEILING
        );
        // Saturating, so a pathological base cannot wrap.
        assert_eq!(
            attempt_budget(u32::MAX, 3, None),
            MEDIA_UNDERSTAND_MAX_TOKENS_CEILING
        );
    }

    /// A limit the model itself declares still caps the doubled budget, and the smaller
    /// of the two limits wins.
    #[test]
    fn retry_budget_never_exceeds_the_declared_cap() {
        assert_eq!(attempt_budget(20480, 3, Some(32768)), 32768);
        assert_eq!(attempt_budget(20480, 2, Some(20480)), 20480);
        // Declared 40960 is below the ceiling, so the declared cap is what applies.
        assert_eq!(attempt_budget(20480, 3, Some(40960)), 40960);
    }

    /// The ladders this batch was sized against, end to end through the real entry point
    /// (`media_understand_max_tokens` → `attempt_budget`).
    #[test]
    fn retry_ladder_for_each_setting_shape() {
        fn ladder(settings: &ModelSettings) -> Vec<u32> {
            let base = media_understand_max_tokens(settings);
            let cap = crate::models::declared_max_tokens(settings);
            (1..=MEDIA_UNDERSTAND_MAX_ATTEMPTS)
                .map(|attempt| attempt_budget(base, attempt, cap))
                .collect()
        }

        // ① media tier thinking on (the field's 4096), nothing declared.
        let mut thinking = settings_with_large_output_cap();
        thinking.round_thinking_budget = Some(4096);
        assert_eq!(crate::models::declared_max_tokens(&thinking), None);
        assert_eq!(ladder(&thinking), vec![20480, 40960, 65536]);

        // ② a declared cap of exactly the ceiling is the same ladder as ① (min of two
        // equal limits) — asserted directly on the clamp, since both limits are 65536.
        assert_eq!(attempt_budget(20480, 3, Some(65536)), 65536);

        // ③ media tier thinking off: the ladder starts at the answer budget.
        let mut no_thinking = settings_with_large_output_cap();
        no_thinking.round_thinking_budget = None;
        assert_eq!(ladder(&no_thinking), vec![16384, 32768, 65536]);

        // ④ a declared cap below the ceiling: attempts 2 and 3 share the same budget —
        // the retry still differs (the truncation note), it just cannot grow.
        assert_eq!(attempt_budget(20480, 2, Some(32768)), 32768);
        assert_eq!(attempt_budget(20480, 3, Some(32768)), 32768);
    }

    #[test]
    fn length_truncation_matches_finish_reason_or_cap() {
        assert!(is_length_truncated(&empty_out(Some("length"), 10), 20480));
        assert!(is_length_truncated(&empty_out(Some("stop"), 20480), 20480));
        assert!(!is_length_truncated(&empty_out(Some("stop"), 12), 20480));
    }

    /// The retry note is appended only when there is one, and the goal stays in place.
    #[test]
    fn retry_note_is_appended_only_when_present() {
        let first = user_content_with_retry_note("Describe the attached image.", "读表格", None);
        assert!(!first.contains(TRUNCATED_RETRY_NOTE), "{first}");
        assert!(first.contains("读表格"), "{first}");

        let retry = user_content_with_retry_note(
            "Describe the attached image.",
            "读表格",
            Some(TRUNCATED_RETRY_NOTE),
        );
        assert!(retry.contains(TRUNCATED_RETRY_NOTE), "{retry}");
        assert!(retry.contains("读表格"), "{retry}");
        assert!(retry.starts_with(&first), "{retry}");
    }

    /// Every path builds its message through this struct, so the retry note has to reach
    /// the wire without disturbing the images / labels the model needs.
    #[test]
    fn prompt_message_carries_images_labels_and_note() {
        let images = vec!["AAA".to_string(), "BBB".to_string()];
        let labels = vec!["a.png".to_string(), "b.png".to_string()];
        let prompt = MediaUnderstandPrompt {
            kind: "image directory understanding",
            call_name: "media_image_dir_understand",
            system_prompt: MULTI_IMAGE_DESCRIBE_PROMPT,
            message_id: "media-images-describe",
            base: "Describe 2 image(s) in /tmp.".into(),
            goal: "读表格",
            images: &images,
            labels: Some(&labels),
            token_kind: MediaUnderstandKind::Image,
        };

        let first = prompt.user_message(None);
        assert_eq!(first.id, "media-images-describe");
        assert_eq!(first.images_base64.as_deref(), Some(images.as_slice()));
        assert_eq!(first.image_slot_labels.as_deref(), Some(labels.as_slice()));
        assert!(!first.content.contains(TRUNCATED_RETRY_NOTE), "{}", first.content);

        let retry = prompt.user_message(Some(TRUNCATED_RETRY_NOTE));
        assert!(retry.content.contains(TRUNCATED_RETRY_NOTE), "{}", retry.content);
        assert!(retry.content.contains("读表格"), "{}", retry.content);
        assert_eq!(retry.images_base64, first.images_base64);
    }
}
