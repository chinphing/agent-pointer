/// XML tool-call parsing: buffer until a full `<response>…</response>`, then parse via
/// [`crate::response_xml::parse_tool_response_default_chain`] (ScraperHtml → relaxed quick-xml).

pub use crate::response_xml::{
    parse_tool_response_default_chain, xml_tool_arguments_to_json_string, ResponseXmlBackend,
    ResponseXmlParseError, XmlToolCall,
};

/// Take the first complete `<response>…</response>` from `buf`, returning `(end_index, fragment)`.
/// If the opening `<response>` is missing but `</response>` exists, wrap the body in a synthetic root.
/// Best-effort: first `<tag ...>...</tag>` inner text (open tag may have attributes).
/// Used when strict XML parsing fails (e.g. unescaped `<` in `<tool_args>`) so UI still gets
/// `<thoughts>` / `<headline>` that were visible while streaming.
fn loose_extract_first_tag_inner(xml: &str, tag: &str) -> Option<String> {
    let open_prefix = format!("<{tag}");
    let open_start = xml.find(&open_prefix)?;
    let after_prefix = xml.get(open_start..)?;
    let open_end_rel = after_prefix.find('>')?;
    let inner_start = open_start + open_end_rel + 1;
    let close_tag = format!("</{tag}>");
    let inner = xml.get(inner_start..)?;
    let close_rel = inner.find(&close_tag)?;
    let body = inner.get(..close_rel)?.trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}

fn extract_response_fragment(buf: &str) -> Option<(usize, String)> {
    if let Some(start) = buf.find("<response>") {
        let after_open = start + "<response>".len();
        let tail = buf.get(after_open..)?;
        let close_rel = tail.find("</response>")?;
        let end = after_open + close_rel + "</response>".len();
        let frag = buf.get(start..end)?.to_string();
        return Some((end, frag));
    }
    // Common case: closing `</response>` without leading `<response>`
    let close = buf.find("</response>")?;
    let end = close + "</response>".len();
    let body = buf.get(..close)?.trim();
    if body.is_empty() {
        return None;
    }
    Some((end, format!("<response>{body}</response>")))
}

/// Diagnostics after the stream ends (for recoverable XML tool failures).
#[derive(Debug, Clone, Default)]
pub struct XmlToolFinishDiagnostics {
    /// `<tool_name>` or `<tool_args>` appeared in assistant content / reasoning stream.
    pub attempted_tool_xml: bool,
    /// A full `</response>` fragment was assembled and parsing was attempted.
    pub fragment_complete: bool,
    /// Parser error when `fragment_complete` but no tool call was produced.
    pub parse_error: Option<String>,
}

/// Streaming-safe parser: buffers until `</response>`, then parses the fragment.
pub struct XmlToolParser {
    buffer: String,
    is_complete: bool,
    current_call: Option<XmlToolCall>,
    last_parse_error: Option<String>,
    fallback_thoughts: Option<String>,
    fallback_headline: Option<String>,
}

impl XmlToolParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            is_complete: false,
            current_call: None,
            last_parse_error: None,
            fallback_thoughts: None,
            fallback_headline: None,
        }
    }

    /// When [`Self::parse`] returns `None` but a `</response>` fragment existed, these may hold
    /// [`loose_extract_first_tag_inner`] results for UI / persistence.
    pub fn take_fallback_thoughts_headline(&mut self) -> (Option<String>, Option<String>) {
        (
            self.fallback_thoughts.take(),
            self.fallback_headline.take(),
        )
    }

    pub fn feed(&mut self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }

        self.buffer.push_str(chunk);
        self.try_finish();
    }

    pub fn is_complete(&self) -> bool {
        self.is_complete
    }

    pub fn parse(&mut self) -> Option<XmlToolCall> {
        if !self.is_complete {
            return None;
        }

        self.current_call.take()
    }

    pub fn last_parse_error(&self) -> Option<&str> {
        self.last_parse_error.as_deref()
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
        self.current_call = None;
        self.is_complete = false;
        self.last_parse_error = None;
        self.fallback_thoughts = None;
        self.fallback_headline = None;
    }

    fn try_finish(&mut self) {
        if self.is_complete {
            return;
        }
        let Some((end, frag)) = extract_response_fragment(&self.buffer) else {
            return;
        };
        match parse_tool_response_default_chain(&frag) {
            Ok(call) => {
                self.last_parse_error = None;
                self.current_call = Some(call);
                self.fallback_thoughts = None;
                self.fallback_headline = None;
            }
            Err(e) => {
                let prefix: String = frag.chars().take(500).collect();
                let truncated = frag.chars().count() > 500;
                log::warn!(
                    "xml_tool_caller: parse_tool_response_default_chain failed (fragment_chars={}{}): {}; prefix={:?}",
                    frag.chars().count(),
                    if truncated { ", truncated in log" } else { "" },
                    e,
                    prefix
                );
                self.last_parse_error = Some(e.to_string());
                self.current_call = None;
                self.fallback_thoughts = loose_extract_first_tag_inner(&frag, "thoughts");
                self.fallback_headline = loose_extract_first_tag_inner(&frag, "headline");
            }
        }
        self.is_complete = true;
        self.buffer.drain(..end);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn test_parse_complete_xml() {
        let mut parser = XmlToolParser::new();
        let xml = r#"<response>
            <thoughts>Handle user request</thoughts>
            <headline>Execute</headline>
            <tool_name>response</tool_name>
            <tool_args>
                <text>Hello</text>
            </tool_args>
        </response>"#;

        parser.feed(xml);
        assert!(parser.is_complete());

        let call = parser.parse().unwrap();
        assert_eq!(call.name, "response");
        assert_eq!(call.thoughts.trim(), "Handle user request");
        assert_eq!(call.headline.trim(), "Execute");
        assert_eq!(call.arguments.get("text").unwrap(), "Hello");
    }

    #[test]
    fn test_parse_missing_open_response_tag() {
        let mut parser = XmlToolParser::new();
        let xml = r#"<thoughts>t</thoughts>
<headline>h</headline>
<tool_name>wait</tool_name>
<tool_args><seconds>1</seconds></tool_args>
</response>"#;
        parser.feed(xml);
        assert!(parser.is_complete());
        let call = parser.parse().unwrap();
        assert_eq!(call.name, "wait");
        assert_eq!(call.arguments.get("seconds").unwrap(), "1");
    }

    #[test]
    fn test_parse_chunked_xml() {
        let mut parser = XmlToolParser::new();

        parser.feed("<response>");
        assert!(!parser.is_complete());

        parser.feed("<tool_name>wait</tool_name>");
        assert!(!parser.is_complete());

        parser.feed("<tool_args><seconds>5</seconds></tool_args>");
        assert!(!parser.is_complete());

        parser.feed("</response>");
        assert!(parser.is_complete());

        let call = parser.parse().unwrap();
        assert_eq!(call.name, "wait");
        assert_eq!(call.arguments.get("seconds").unwrap(), "5");
    }

    #[test]
    fn test_parse_minimal_xml() {
        let mut parser = XmlToolParser::new();
        parser.feed("<response><tool_name>test</tool_name></response>");

        let call = parser.parse().unwrap();
        assert_eq!(call.name, "test");
        assert!(call.arguments.is_empty());
    }

    #[test]
    fn test_parse_multiple_args() {
        let mut parser = XmlToolParser::new();
        let xml = r#"<response>
            <tool_name>custom</tool_name>
            <tool_args>
                <arg1>value1</arg1>
                <arg2>value2</arg2>
            </tool_args>
        </response>"#;

        parser.feed(xml);
        let call = parser.parse().unwrap();
        assert_eq!(call.arguments.get("arg1").unwrap(), "value1");
        assert_eq!(call.arguments.get("arg2").unwrap(), "value2");
    }

    #[test]
    fn test_parse_file_edit_with_cdata_embedded_markup() {
        let mut parser = XmlToolParser::new();
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <path>src/App.vue</path>
    <oldString><![CDATA[  <div v-if="ok">x</div>  ]]></oldString>
    <newString><![CDATA[  <div v-if="ok">y</div>  ]]></newString>
  </tool_args>
</response>"#;
        parser.feed(xml);
        let call = parser.parse().unwrap();
        assert_eq!(call.name, "file:edit");
        assert_eq!(call.arguments.get("path").map(String::as_str), Some("src/App.vue"));
        assert!(call.arguments.get("oldString").unwrap().contains("v-if"));
        assert!(call.arguments.get("newString").unwrap().contains("v-if"));
    }

    #[test]
    fn test_reset_parser() {
        let mut parser = XmlToolParser::new();
        parser.feed("<response><tool_name>test1</tool_name></response>");
        assert!(parser.is_complete());

        parser.reset();
        assert!(!parser.is_complete());

        parser.feed("<response><tool_name>test2</tool_name></response>");
        let call = parser.parse().unwrap();
        assert_eq!(call.name, "test2");
    }

    #[test]
    fn test_parse_single_chunk_full_response() {
        let mut parser = XmlToolParser::new();
        let xml = "<response><tool_name>wait</tool_name><tool_args><seconds>5</seconds></tool_args></response>";
        parser.feed(xml);
        assert!(parser.is_complete());
        let call = parser.parse().unwrap();
        assert_eq!(call.name, "wait");
        assert_eq!(call.arguments.get("seconds").map(String::as_str), Some("5"));
    }

    #[test]
    fn test_xml_args_json_coercion() {
        let mut m = HashMap::new();
        m.insert("seconds".into(), "5".into());
        m.insert("flag".into(), "true".into());
        m.insert("label".into(), "hello".into());
        let j = xml_tool_arguments_to_json_string(&m);
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["seconds"], 5);
        assert_eq!(v["flag"], true);
        assert_eq!(v["label"], "hello");
    }

    #[test]
    fn test_xml_args_json_array_for_hotkey_keys() {
        let mut m = HashMap::new();
        m.insert("keys".into(), r#"["command", "space"]"#.into());
        let j = xml_tool_arguments_to_json_string(&m);
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        let arr = v["keys"].as_array().expect("keys must be JSON array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].as_str(), Some("command"));
        assert_eq!(arr[1].as_str(), Some("space"));
    }

    #[test]
    fn loose_extract_finds_thoughts_with_attribute_on_open_tag() {
        let xml = r#"<response><thoughts lang="x">inner</thoughts></response>"#;
        assert_eq!(
            loose_extract_first_tag_inner(xml, "thoughts").as_deref(),
            Some("inner")
        );
    }

    /// Bare `<` in `oldString` breaks strict quick-xml; default chain uses ScraperHtml first and succeeds.
    #[test]
    fn parser_default_chain_parses_unescaped_lt_in_old_string() {
        let mut parser = XmlToolParser::new();
        let xml = r#"<response>
  <thoughts>Planning edit</thoughts>
  <headline>Patch file</headline>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <path>x.txt</path>
    <oldString>if a < b</oldString>
    <newString>z</newString>
  </tool_args>
</response>"#;
        parser.feed(xml);
        assert!(parser.is_complete());
        let call = parser.parse().expect("ScraperHtml should parse this fragment");
        assert_eq!(call.thoughts.trim(), "Planning edit");
        assert_eq!(call.headline.trim(), "Patch file");
        assert_eq!(call.name, "file:edit");
        assert_eq!(call.arguments.get("oldString").map(String::as_str), Some("if a < b"));
        assert_eq!(call.arguments.get("newString").map(String::as_str), Some("z"));
    }
}
