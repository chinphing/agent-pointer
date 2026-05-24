//! AGENT.md SearchAgent section only (no COMMUNICATION.md) for inner web_search system message.

const RESEARCH_AGENT_MD: &str = include_str!("../AGENT.md");

/// Markdown body for SearchAgent: `## SearchAgent` section from `research/AGENT.md`.
pub fn research_agent_md_body() -> &'static str {
    static BODY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BODY.get_or_init(|| extract_search_agent_section(RESEARCH_AGENT_MD))
        .as_str()
}

fn extract_md_body(raw: &str) -> String {
    let text = raw.trim_start_matches('\u{feff}');
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return text.trim().to_string();
    }
    let mut body = Vec::new();
    let mut in_body = false;
    for line in lines {
        if !in_body {
            if line.trim() == "---" {
                in_body = true;
            }
            continue;
        }
        body.push(line);
    }
    body.join("\n").trim().to_string()
}

fn extract_search_agent_section(raw: &str) -> String {
    let body = extract_md_body(raw);
    let marker = "## SearchAgent";
    let orchestrator = "## Orchestrator";
    let start = body.find(marker).unwrap_or(0);
    let rest = &body[start..];
    let end = rest.find(orchestrator).unwrap_or(rest.len());
    rest[..end].trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_is_search_agent_section_only() {
        let body = research_agent_md_body();
        assert!(body.contains("SearchAgent"));
        assert!(body.contains("web-only research"));
        assert!(!body.contains("Orchestrator"));
        assert!(!body.contains("id: research"));
    }
}
