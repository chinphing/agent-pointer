use crate::models::ModelSettings;

/// Heuristic: whether the active model accepts vision (`image_url`) input on chat/completions.
pub fn model_supports_vision(settings: &ModelSettings) -> bool {
    let model = settings.model.trim().to_ascii_lowercase();
    if model.is_empty() {
        return false;
    }
    if model.contains("-vl-") || model.contains("omni") {
        return true;
    }
    for stem in [
        "qwen3.5-plus",
        "qwen3.5-flash",
        "qwen3.6-plus",
        "qwen3.6-flash",
        "qwen3.7-max",
        "qwen-vl",
        "gpt-4o",
        "gpt-4.1",
        "claude-3",
        "claude-sonnet-4",
        "claude-opus-4",
    ] {
        if model.starts_with(stem) || model.contains(stem) {
            return true;
        }
    }
    false
}
