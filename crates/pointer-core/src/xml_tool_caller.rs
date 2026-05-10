/// XML tool-call parsing: buffer until a full `<response>…</response>`, then parse with `quick-xml`.

use std::collections::HashMap;

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use serde_json::{Map, Number, Value};

#[derive(Debug, Clone)]
pub struct XmlToolCall {
    pub name: String,
    pub arguments: HashMap<String, String>,
    pub thoughts: String,
    pub headline: String,
}

/// Convert XML string arguments to JSON for `parse_tool_call_arguments` / tool handlers.
/// Coerces numbers/bools and JSON array/object literals embedded in text nodes.
pub fn xml_tool_arguments_to_json_string(args: &HashMap<String, String>) -> String {
    let map: Map<String, Value> = args
        .iter()
        .map(|(k, v)| (k.clone(), coerce_xml_text_to_json_value(v)))
        .collect();
    Value::Object(map).to_string()
}

fn coerce_xml_text_to_json_value(s: &str) -> Value {
    let t = s.trim();
    if t.is_empty() {
        return Value::String(s.to_string());
    }
    // Models often embed JSON literals (e.g. hotkey keys array); parse as structured JSON when valid.
    if t.starts_with('{') || t.starts_with('[') {
        if let Ok(v) = serde_json::from_str::<Value>(t) {
            return v;
        }
    }
    if t == "true" {
        return Value::Bool(true);
    }
    if t == "false" {
        return Value::Bool(false);
    }
    if t == "null" {
        return Value::Null;
    }
    if let Ok(i) = t.parse::<i64>() {
        return Value::Number(i.into());
    }
    if let Ok(f) = t.parse::<f64>() {
        if let Some(n) = Number::from_f64(f) {
            return Value::Number(n);
        }
    }
    Value::String(s.to_string())
}

/// Take the first complete `<response>…</response>` from `buf`, returning `(end_index, fragment)`.
/// If the opening `<response>` is missing but `</response>` exists, wrap the body in a synthetic root.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextSlot {
    Idle,
    Thoughts,
    Headline,
    ToolName,
    ToolArgs,
    Arg,
}

fn parse_response_xml(xml: &str) -> Result<XmlToolCall, quick_xml::Error> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut call = XmlToolCall {
        name: String::new(),
        arguments: HashMap::new(),
        thoughts: String::new(),
        headline: String::new(),
    };

    let mut slot = TextSlot::Idle;
    let mut text_buf = String::new();
    let mut arg_name = String::new();

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(e) => {
                text_buf.clear();
                match e.name().local_name().as_ref() {
                    b"response" => {}
                    b"thoughts" => slot = TextSlot::Thoughts,
                    b"headline" => slot = TextSlot::Headline,
                    b"tool_name" => slot = TextSlot::ToolName,
                    b"tool_args" => slot = TextSlot::ToolArgs,
                    name if slot == TextSlot::ToolArgs => {
                        arg_name = String::from_utf8_lossy(name).into_owned();
                        slot = TextSlot::Arg;
                    }
                    _ => {}
                }
            }
            Event::Empty(e) => {
                if slot == TextSlot::ToolArgs {
                    let name = String::from_utf8_lossy(e.name().local_name().as_ref()).into_owned();
                    call.arguments.insert(name, String::new());
                }
            }
            Event::Text(e) => {
                // Do not use `unescape()`: models emit raw `&`, `<`, etc. in text nodes; strict entity
                // decoding fails on those. Treat the text segment as UTF-8 bytes (tool payloads use CDATA
                // when markup would break tag structure).
                let t = String::from_utf8_lossy(e.as_ref()).into_owned();
                match slot {
                    TextSlot::Thoughts
                    | TextSlot::Headline
                    | TextSlot::ToolName
                    | TextSlot::Arg => text_buf.push_str(&t),
                    TextSlot::Idle | TextSlot::ToolArgs => {}
                }
            }
            Event::CData(e) => {
                let t = String::from_utf8_lossy(e.as_ref()).into_owned();
                match slot {
                    TextSlot::Thoughts
                    | TextSlot::Headline
                    | TextSlot::ToolName
                    | TextSlot::Arg => text_buf.push_str(&t),
                    TextSlot::Idle | TextSlot::ToolArgs => {}
                }
            }
            Event::End(e) => {
                match e.name().local_name().as_ref() {
                    b"thoughts" if slot == TextSlot::Thoughts => {
                        call.thoughts = std::mem::take(&mut text_buf);
                        slot = TextSlot::Idle;
                    }
                    b"headline" if slot == TextSlot::Headline => {
                        call.headline = std::mem::take(&mut text_buf);
                        slot = TextSlot::Idle;
                    }
                    b"tool_name" if slot == TextSlot::ToolName => {
                        call.name = std::mem::take(&mut text_buf).trim().to_string();
                        slot = TextSlot::Idle;
                    }
                    b"tool_args" if slot == TextSlot::ToolArgs => {
                        slot = TextSlot::Idle;
                    }
                    end_name
                        if slot == TextSlot::Arg
                            && end_name == arg_name.as_bytes() =>
                    {
                        call.arguments
                            .insert(std::mem::take(&mut arg_name), std::mem::take(&mut text_buf));
                        slot = TextSlot::ToolArgs;
                    }
                    _ => {}
                }
            }
            Event::Decl(_) | Event::PI(_) | Event::DocType(_) | Event::Comment(_) => {}
        }
    }

    Ok(call)
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
}

impl XmlToolParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            is_complete: false,
            current_call: None,
            last_parse_error: None,
        }
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
    }

    fn try_finish(&mut self) {
        if self.is_complete {
            return;
        }
        let Some((end, frag)) = extract_response_fragment(&self.buffer) else {
            return;
        };
        match parse_response_xml(&frag) {
            Ok(call) => {
                self.last_parse_error = None;
                self.current_call = Some(call);
            }
            Err(e) => {
                let prefix: String = frag.chars().take(500).collect();
                let truncated = frag.chars().count() > 500;
                log::warn!(
                    "xml_tool_caller: parse_response_xml failed (fragment_chars={}{}): {}; prefix={:?}",
                    frag.chars().count(),
                    if truncated { ", truncated in log" } else { "" },
                    e,
                    prefix
                );
                self.last_parse_error = Some(e.to_string());
                self.current_call = None;
            }
        }
        self.is_complete = true;
        self.buffer.drain(..end);
    }
}

#[cfg(test)]
mod tests {
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
  <tool_name>file_edit</tool_name>
  <tool_args>
    <path>src/App.vue</path>
    <oldString><![CDATA[  <div v-if="ok">x</div>  ]]></oldString>
    <newString><![CDATA[  <div v-if="ok">y</div>  ]]></newString>
  </tool_args>
</response>"#;
        parser.feed(xml);
        let call = parser.parse().unwrap();
        assert_eq!(call.name, "file_edit");
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

    /// Text nodes use raw UTF-8 (no `unescape()`), so a bare `&` is kept as-is.
    #[test]
    fn parse_response_xml_accepts_bare_ampersand_in_tool_arg() {
        let xml = r#"<response>
  <tool_name>file_edit</tool_name>
  <tool_args>
    <path>x.txt</path>
    <oldString>foo & bar</oldString>
    <newString>ok</newString>
  </tool_args>
</response>"#;
        let call = parse_response_xml(xml).expect("parse");
        assert_eq!(call.arguments.get("oldString").map(String::as_str), Some("foo & bar"));
    }

    /// We do not decode XML entities in `Event::Text`; `&amp;` stays literal (prefer CDATA or raw `&`).
    #[test]
    fn parse_response_xml_text_leaves_amp_entity_literal() {
        let xml = r#"<response>
  <tool_name>file_edit</tool_name>
  <tool_args>
    <path>x.txt</path>
    <oldString>foo &amp; bar</oldString>
    <newString>z</newString>
  </tool_args>
</response>"#;
        let call = parse_response_xml(xml).expect("parse");
        assert_eq!(
            call.arguments.get("oldString").map(String::as_str),
            Some("foo &amp; bar")
        );
    }
}
