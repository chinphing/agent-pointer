/// Markdown **## Tools** appendix for the system prompt (per-tool `doc_markdown` only).
use crate::tools::ToolRegistry;
use std::collections::HashSet;

fn append_tool_doc(out: &mut String, name: &str, doc_markdown: &str) {
    let desc = doc_markdown.trim();
    if desc.starts_with("###") {
        out.push_str(desc);
        out.push_str("\n\n");
    } else {
        out.push_str(&format!("### {name}\n"));
        out.push_str(&format!("Description: {desc}\n"));
        out.push('\n');
    }
}

/// Build the enabled-tool list. Argument shapes and examples live entirely in each tool's `doc_markdown`.
/// Multiple tools sharing the same [`ToolEntry::doc_source`] prompt file emit one appendix block.
pub fn generate_tools_system_appendix(tools: &ToolRegistry, allow: &[String]) -> String {
    let filtered_tools = tools.xml_tool_descriptors(allow);

    if filtered_tools.is_empty() {
        return String::new();
    }

    let mut out = String::from("## Tools\n\n");
    let mut seen_sources = std::collections::HashSet::new();

    for tool in &filtered_tools {
        if !seen_sources.insert(tool.doc_source.clone()) {
            continue;
        }
        append_tool_doc(&mut out, &tool.name, &tool.doc_markdown);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolEntry;
    use std::sync::Arc;

    const MOUSE_DOC_SOURCE: &str = "agents/computer/tools/prompts/mouse.md";

    #[test]
    fn dedupes_by_doc_source_for_tool_families() {
        let reg = ToolRegistry::new();
        let shared = "### mouse family\nShared mouse doc.";
        for name in ["mouse_click_index", "mouse_click_at", "mouse_hover_index"] {
            reg.register(ToolEntry::new(
                name,
                MOUSE_DOC_SOURCE,
                "low",
                false,
                shared,
                Arc::new(|_| Ok(String::new())),
            ));
        }

        let allow = vec![
            "mouse_click_index".into(),
            "mouse_click_at".into(),
            "mouse_hover_index".into(),
        ];
        let appendix = generate_tools_system_appendix(&reg, &allow);

        assert_eq!(appendix.matches("Shared mouse doc.").count(), 1);
        assert_eq!(appendix.matches("### mouse family").count(), 1);
    }

    #[test]
    fn keeps_distinct_docs_with_different_sources() {
        let reg = ToolRegistry::new();
        reg.register(ToolEntry::new(
            "alpha",
            "tools/prompts/alpha.md",
            "low",
            false,
            "Alpha description.",
            Arc::new(|_| Ok(String::new())),
        ));
        reg.register(ToolEntry::new(
            "beta",
            "tools/prompts/beta.md",
            "low",
            false,
            "Beta description.",
            Arc::new(|_| Ok(String::new())),
        ));

        let allow = vec!["alpha".into(), "beta".into()];
        let appendix = generate_tools_system_appendix(&reg, &allow);

        assert!(appendix.contains("### alpha"));
        assert!(appendix.contains("### beta"));
        assert!(appendix.contains("Alpha description."));
        assert!(appendix.contains("Beta description."));
    }

    #[test]
    fn identical_content_different_sources_emits_both() {
        let reg = ToolRegistry::new();
        let text = "Same body.";
        reg.register(ToolEntry::new(
            "tool_a",
            "tools/prompts/a.md",
            "low",
            false,
            text,
            Arc::new(|_| Ok(String::new())),
        ));
        reg.register(ToolEntry::new(
            "tool_b",
            "tools/prompts/b.md",
            "low",
            false,
            text,
            Arc::new(|_| Ok(String::new())),
        ));

        let allow = vec!["tool_a".into(), "tool_b".into()];
        let appendix = generate_tools_system_appendix(&reg, &allow);

        assert_eq!(appendix.matches("Same body.").count(), 2);
    }

    #[test]
    fn empty_when_no_tools_enabled() {
        let reg = ToolRegistry::new();
        assert!(generate_tools_system_appendix(&reg, &[]).is_empty());
    }
}
