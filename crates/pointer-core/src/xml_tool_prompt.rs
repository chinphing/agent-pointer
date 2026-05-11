/// Dynamic tools appendix for the model (per-tool markdown only).

use crate::tools::ToolRegistry;

/// Build the enabled-tool list. Argument shapes and examples live entirely in each tool’s `doc_markdown`.
pub fn generate_xml_tool_prompt(tools: &ToolRegistry, allow: &[String]) -> String {
    let filtered_tools = tools.xml_tool_descriptors(allow);

    if filtered_tools.is_empty() {
        return String::new();
    }

    let mut prompt = String::from("## Tools\n\n");

    for tool in &filtered_tools {
        let desc = tool.doc_markdown.trim();
        // Computer tools ship markdown with `### name`; avoid duplicating the heading.
        if desc.starts_with("###") {
            prompt.push_str(desc);
            prompt.push_str("\n\n");
        } else {
            prompt.push_str(&format!("### {}\n", tool.name));
            prompt.push_str(&format!("Description: {}\n", desc));
        }
        prompt.push('\n');
    }

    prompt
}
