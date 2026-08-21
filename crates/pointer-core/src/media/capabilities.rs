use crate::models::{model_capability_flags, ModelSettings};

/// Whether `provider_id`/`model` is catalogued for speech-to-text.
/// Unset means no; do not infer from the model name.
pub fn model_supports_audio_transcription(
    settings: &ModelSettings,
    provider_id: &str,
    model: &str,
) -> bool {
    let mid = model.trim();
    if mid.is_empty() {
        return false;
    }
    settings
        .providers
        .iter()
        .find(|p| p.id == provider_id)
        .and_then(|p| p.model_configs.get(mid))
        .and_then(|o| o.supports_audio)
        .unwrap_or(false)
}

/// Whether the active model accepts vision (`image_url`) input on chat/completions.
pub fn model_supports_vision(settings: &ModelSettings) -> bool {
    let model = settings.model.trim();
    if model.is_empty() {
        return false;
    }
    let pid = settings.active_provider_id.trim();
    let (vision, _, _) = model_capability_flags(settings, pid, model);
    vision
}
