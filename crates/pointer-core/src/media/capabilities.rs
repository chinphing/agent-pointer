use crate::models::{model_capability_flags, ModelSettings};

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
