/// Markdown **## Tools** appendix for the system prompt (per-tool `doc_markdown` only).

use crate::tools::ToolRegistry;

/// Build the enabled-tool list. Argument shapes and examples live entirely in each tool’s `doc_markdown`.
pub fn generate_tools_system_appendix(tools: &ToolRegistry, allow: &[String]) -> String {
    let filtered_tools = tools.xml_tool_descriptors(allow);

    if filtered_tools.is_empty() {
        return String::new();
    }

    let mut out = String::from("## Tools\n\n");

    for tool in &filtered_tools {
        let desc = tool.doc_markdown.trim();
        // Computer tools ship markdown with `### name`; avoid duplicating the heading.
        if desc.starts_with("###") {
            out.push_str(desc);
            out.push_str("\n\n");
        } else {
            out.push_str(&format!("### {}\n", tool.name));
            out.push_str(&format!("Description: {}\n", desc));
        }
        out.push('\n');
    }

    out
}
