//! User-configured coding rules injected into LLM system cacheable prompts.

pub const USER_CODING_RULES_MAX_CHARS: usize = 4000;

/// Append `[USER RULES]` block when non-empty (trimmed, capped).
pub fn push_user_coding_rules_to_cacheable(cacheable: &mut Vec<String>, rules: &str) {
    let trimmed = rules.trim();
    if trimmed.is_empty() {
        return;
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let body = if chars.len() > USER_CODING_RULES_MAX_CHARS {
        let head: String = chars
            .iter()
            .take(USER_CODING_RULES_MAX_CHARS)
            .collect();
        format!("{head}\n…(truncated at {USER_CODING_RULES_MAX_CHARS} chars)")
    } else {
        trimmed.to_string()
    };
    cacheable.push(format!("[USER RULES]\n{body}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rules_not_injected() {
        let mut c = Vec::new();
        push_user_coding_rules_to_cacheable(&mut c, "  \n  ");
        assert!(c.is_empty());
    }

    #[test]
    fn non_empty_rules_injected_with_header() {
        let mut c = Vec::new();
        push_user_coding_rules_to_cacheable(&mut c, "- minimal diff");
        assert_eq!(c.len(), 1);
        assert!(c[0].starts_with("[USER RULES]\n"));
        assert!(c[0].contains("minimal diff"));
    }

    #[test]
    fn long_rules_truncated() {
        let mut c = Vec::new();
        let long = "x".repeat(USER_CODING_RULES_MAX_CHARS + 50);
        push_user_coding_rules_to_cacheable(&mut c, &long);
        assert!(c[0].contains("truncated"));
    }
}
