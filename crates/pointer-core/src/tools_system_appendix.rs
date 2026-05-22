/// Markdown **## Tools** appendix for the system prompt (per-tool `doc_markdown` only).

use crate::agents::computer::tools::tool_prompts::{
    computer_tool_doc_override, ComputerPositioningMode,
};
use crate::tools::ToolRegistry;

/// Build the enabled-tool list. Argument shapes and examples live entirely in each tool’s `doc_markdown`.
pub fn generate_tools_system_appendix(tools: &ToolRegistry, allow: &[String]) -> String {
    generate_tools_system_appendix_with_positioning(tools, allow, None)
}

/// Same as [`generate_tools_system_appendix`], with optional computer index vs coordinate tool docs.
pub fn generate_tools_system_appendix_with_positioning(
    tools: &ToolRegistry,
    allow: &[String],
    computer_positioning: Option<ComputerPositioningMode>,
) -> String {
    let mut filtered_tools = tools.xml_tool_descriptors(allow);
    if let Some(mode) = computer_positioning {
        for d in &mut filtered_tools {
            if let Some(doc) = computer_tool_doc_override(&d.name, mode) {
                d.doc_markdown = doc.to_string();
            }
        }
    }

    if filtered_tools.is_empty() {
        return String::new();
    }

    let mut out = String::from("## Tools\n\n");

    for tool in &filtered_tools {
        let desc = tool.doc_markdown.trim();
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
