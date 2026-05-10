/// XML tool-calling system prompt generation.

use crate::tools::ToolRegistry;

/// Build the XML tool-calling appendix for the model (tool list + shape + examples).
pub fn generate_xml_tool_prompt(tools: &ToolRegistry, allow: &[String]) -> String {
    let filtered_tools = tools.xml_tool_descriptors(allow);

    if filtered_tools.is_empty() {
        return String::new();
    }

    let mut prompt = String::from("## Tool calls (XML)\n\n");
    prompt.push_str("When you need a tool, use exactly this XML shape:\n\n");
    prompt.push_str("```xml\n");
    prompt.push_str("<response>\n");
    prompt.push_str("  <thoughts>Your reasoning</thoughts>\n");
    prompt.push_str("  <headline>Short step title</headline>\n");
    prompt.push_str("  <tool_name>tool_name</tool_name>\n");
    prompt.push_str("  <tool_args>\n");
    prompt.push_str("    <arg_name>value</arg_name>\n");
    prompt.push_str("  </tool_args>\n");
    prompt.push_str("</response>\n");
    prompt.push_str("```\n\n");
    prompt.push_str("## Available tools\n\n");

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

        if let Some(schema) = tool.parameters_schema.as_object() {
            if let Some(props) = schema.get("properties") {
                if let Some(props_obj) = props.as_object() {
                    if !props_obj.is_empty() {
                        prompt.push_str("Parameters:\n");
                        for (param_name, param_def) in props_obj {
                            let param_type = param_def
                                .get("type")
                                .and_then(|t| t.as_str())
                                .unwrap_or("string");
                            let param_desc = param_def
                                .get("description")
                                .and_then(|d| d.as_str())
                                .unwrap_or("");
                            prompt.push_str(&format!(
                                "- `{}` ({}): {}\n",
                                param_name, param_type, param_desc
                            ));
                        }
                    }
                }
            }
        }
        prompt.push('\n');
    }

    prompt.push_str("## Examples\n\n");
    prompt.push_str("Example 1 — reply to user:\n");
    prompt.push_str("```xml\n");
    prompt.push_str("<response>\n");
    prompt.push_str("  <thoughts>Done; replying to the user.</thoughts>\n");
    prompt.push_str("  <headline>Reply</headline>\n");
    prompt.push_str("  <tool_name>response</tool_name>\n");
    prompt.push_str("  <tool_args>\n");
    prompt.push_str("    <text>This is the reply body.</text>\n");
    prompt.push_str("  </tool_args>\n");
    prompt.push_str("</response>\n");
    prompt.push_str("```\n\n");

    prompt.push_str("Example 2 — mouse with qualified method (Computer):\n");
    prompt.push_str("```xml\n");
    prompt.push_str("<response>\n");
    prompt.push_str("  <thoughts>Target has an overlay index.</thoughts>\n");
    prompt.push_str("  <headline>Click control</headline>\n");
    prompt.push_str("  <tool_name>mouse:click_index</tool_name>\n");
    prompt.push_str("  <tool_args>\n");
    prompt.push_str("    <goal>Activate the highlighted button</goal>\n");
    prompt.push_str("    <action>click the blue primary button labeled Save in the dialog footer</action>\n");
    prompt.push_str("    <index>7</index>\n");
    prompt.push_str("  </tool_args>\n");
    prompt.push_str("</response>\n");
    prompt.push_str("```\n\n");

    prompt.push_str("Rules:\n");
    prompt.push_str("1. Tool calls must use this XML format.\n");
    prompt.push_str("2. Only one tool per turn.\n");
    prompt.push_str("3. Put arguments inside `<tool_args>` as child elements.\n");
    prompt.push_str("4. Argument names must match the tool definition.\n");
    prompt.push_str("5. Do not add extra text outside the `<response>` block.\n");
    prompt.push_str("6. For `file_write` / `file_edit` XML calls, always wrap `content` / `oldString` / `newString` in CDATA (see tool descriptions and agent communication).\n");

    prompt
}
