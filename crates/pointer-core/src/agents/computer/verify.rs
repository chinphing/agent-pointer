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
        format!(
            "Attempted hotkey action: {}. Do not assume success. Verify the UI effect on the next screenshot.",
            keys.join("+")
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
}

impl Default for VerifyHintGenerator {
    fn default() -> Self {
        Self::new()
    }
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
        assert!(hint.contains("Attempted hotkey action"));
    }

    #[test]
    fn test_wait_hint() {
        let gen = VerifyHintGenerator::new();
        let hint = gen.wait_hint_secs(3.0);
        assert!(hint.contains("waited 3"));
    }
}
