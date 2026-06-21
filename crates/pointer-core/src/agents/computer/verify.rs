/// Generates verification hints after tool execution.
///
/// These hints guide the model to verify the result of its actions
/// in the next screenshot.
#[derive(Debug, Clone)]
pub struct VerifyHintGenerator;

impl VerifyHintGenerator {
    /// Create a new VerifyHintGenerator.
    pub fn new() -> Self {
        Self
    }

    /// Generate a verification hint for a click action.
    ///
    /// # Arguments
    /// * `index` - Optional index that was clicked.
    /// * `coords` - Optional coordinates that were clicked.
    pub fn click_hint(&self, index: Option<u32>, coords: Option<(i32, i32)>) -> String {
        match (index, coords) {
            (Some(_), _) => "Attempted overlay click (index-targeted). Do not assume success. Verify on the next screenshot using visible UI cues only; in verify/repetition reasoning, do not use overlay index numbers."
                .to_string(),
            (_, Some((x, y))) => format!(
                "Attempted click at coordinates ({}, {}). Do not assume success. Verify the result on the next screenshot.",
                x, y
            ),
            _ => "Attempted click action. Do not assume success. Verify the result on the next screenshot.".to_string(),
        }
    }

    /// Generate a verification hint for a scroll action.
    pub fn scroll_hint(&self, lines: i32) -> String {
        let direction = if lines > 0 { "up" } else { "down" };
        format!(
            "Attempted scroll: {} lines {}. Do not assume success. Runtime does not detect movement; compare previous vs current screenshots to verify scroll effect.",
            lines.abs(), direction
        )
    }

    /// Generate a verification hint for a type action.
    pub fn type_hint(&self, text: &str) -> String {
        format!(
            "Attempted text input action with payload '{}'. Do not assume text appeared. Verify the field content on the next screenshot.",
            text
        )
    }

    /// Generate a verification hint for a hotkey action.
    pub fn hotkey_hint(&self, keys: &[&str]) -> String {
        let joined = keys.join("+");
        if is_copy_hotkey(keys) {
            return format!(
                "Attempted hotkey action: {joined}. Copy may have succeeded — on the next turn call clipboard_read to confirm content. Do not repeat Copy without reading the clipboard first."
            );
        }
        format!(
            "Attempted hotkey action: {joined}. Do not assume success. Verify the UI effect on the next screenshot."
        )
    }

    /// Generate a verification hint for a wait action.
    pub fn wait_hint_secs(&self, seconds: f64) -> String {
        format!(
            "Goal step complete: waited {seconds} seconds. Proceed with next action."
        )
    }

    /// Generate a generic verification hint.
    pub fn generic_hint(&self, action_name: &str) -> String {
        format!(
            "Attempted action: {}. Do not assume success. Verify the result on the next screenshot.",
            action_name
        )
    }

    /// Generate a verification hint after list_apps.
    pub fn list_apps_hint(&self, count: usize, include_all: bool) -> String {
        if include_all {
            format!(
                "Listed {count} app(s) (full installed catalog). \
                 Pick the target identifier from the lines above, then call launch_app."
            )
        } else {
            format!(
                "Listed {count} app(s) (running + 14-day recent; Windows also includes Start Menu). \
                 Pick the target from the lines above, then call launch_app. \
                 If the target is missing on macOS/Linux, retry list_apps once with include_all: true — \
                 do not set include_all on the first call."
            )
        }
    }

    /// Generate a verification hint after launch_app.
    pub fn launch_app_hint(&self, app: &str, success: bool, action: &str) -> String {
        if success {
            format!(
                "launch_app {action} for \"{app}\" reported success with host verification."
            )
        } else {
            format!(
                "launch_app {action} for \"{app}\" failed host verification. \
                 Do not assume the app opened — retry launch_app, call list_apps, or use a UI fallback."
            )
        }
    }
}

impl Default for VerifyHintGenerator {
    fn default() -> Self {
        Self::new()
    }
}

fn is_copy_hotkey(keys: &[&str]) -> bool {
    let norm: Vec<String> = keys
        .iter()
        .map(|k| k.trim().to_ascii_lowercase())
        .filter(|k| !k.is_empty())
        .collect();
    let has_c = norm.iter().any(|k| k == "c");
    let has_mod = norm.iter().any(|k| {
        matches!(
            k.as_str(),
            "command" | "cmd" | "ctrl" | "control" | "meta" | "super" | "win"
        )
    });
    has_c && has_mod
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_click_hint_with_index() {
        let gen = VerifyHintGenerator::new();
        let hint = gen.click_hint(Some(3), None);
        assert!(hint.contains("Attempted overlay click"));
        assert!(hint.contains("Verify"));
        assert!(!hint.contains("index 3"));
    }

    #[test]
    fn test_click_hint_with_coords() {
        let gen = VerifyHintGenerator::new();
        let hint = gen.click_hint(None, Some((100, 200)));
        assert!(hint.contains("(100, 200)"));
        assert!(hint.contains("Do not assume success"));
    }

    #[test]
    fn test_click_hint_generic() {
        let gen = VerifyHintGenerator::new();
        let hint = gen.click_hint(None, None);
        assert!(hint.contains("Attempted click action"));
    }

    #[test]
    fn test_scroll_hint() {
        let gen = VerifyHintGenerator::new();
        let hint = gen.scroll_hint(-5);
        assert!(hint.contains("Attempted scroll: 5 lines down"));
        assert!(hint.contains("Do not assume success"));
    }

    #[test]
    fn test_type_hint() {
        let gen = VerifyHintGenerator::new();
        let hint = gen.type_hint("hello world");
        assert!(hint.contains("payload 'hello world'"));
        assert!(hint.contains("Do not assume text appeared"));
    }

    #[test]
    fn test_hotkey_hint() {
        let gen = VerifyHintGenerator::new();
        let hint = gen.hotkey_hint(&["command", "c"]);
        assert!(hint.contains("command+c"));
        assert!(hint.contains("clipboard_read"));
    }

    #[test]
    fn test_copy_hotkey_detection() {
        assert!(is_copy_hotkey(&["command", "c"]));
        assert!(is_copy_hotkey(&["ctrl", "c"]));
        assert!(!is_copy_hotkey(&["command", "v"]));
        assert!(!is_copy_hotkey(&["l"]));
    }

    #[test]
    fn test_wait_hint() {
        let gen = VerifyHintGenerator::new();
        let hint = gen.wait_hint_secs(3.0);
        assert!(hint.contains("waited 3"));
    }
}
