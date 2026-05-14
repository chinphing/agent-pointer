//! Pluggable parsers for the assistant `<response>…</response>` tool XML fragment.
//!
//! - [`parse_tool_response_default_chain`] order: [`ResponseXmlBackend::ScraperHtml`] first (handles
//!   bare `<` / `&` in tool-arg text that strict XML rejects), then **relaxed** `quick-xml` as fallback.
//! - [`ResponseXmlBackend::QuickXml`] is still exposed for callers that want strict XML only.
//! - [`ResponseXmlBackend::ScraperHtml`] — HTML5 fragment parsing; CDATA pre-escape; `oldString` /
//!   `newString` remapped from lowercase tag names.

mod quick_xml;
mod scraper_html;

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;

/// Match [`crate::xml_tool_caller::extract_response_fragment`]: allow `</response>` without `<response>`.
pub(super) fn wrap_synthetic_response_root(input: &str) -> Cow<'_, str> {
    if input.contains("<response>") || input.contains("<response ") {
        return Cow::Borrowed(input);
    }
    let Some(close) = input.find("</response>") else {
        return Cow::Borrowed(input);
    };
    let body = input[..close].trim();
    if body.is_empty() {
        Cow::Borrowed(input)
    } else {
        Cow::Owned(format!("<response>{body}</response>"))
    }
}

use serde_json::{Map, Number, Value};

/// Parsed `<response>`: optional `<sidecar_tools>` calls plus the single root primary tool call.
#[derive(Debug, Clone)]
pub struct XmlToolEnvelope {
    pub sidecar: Vec<XmlToolCall>,
    pub primary: XmlToolCall,
}

/// Parsed `<response>` tool call for the XML tool-caller path.
#[derive(Debug, Clone)]
pub struct XmlToolCall {
    pub name: String,
    pub arguments: HashMap<String, String>,
    pub thoughts: String,
    pub headline: String,
}

/// Which parser implementation to use for [`ResponseXmlBackend::parse_tool_response`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseXmlBackend {
    QuickXml,
    /// HTML5 fragment tree (more tolerant of bare `<` / `&` in text than strict XML).
    ScraperHtml,
}

#[derive(Debug)]
pub enum ResponseXmlParseError {
    QuickXml(String),
    Scraper(String),
}

impl fmt::Display for ResponseXmlParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResponseXmlParseError::QuickXml(e) => write!(f, "{e}"),
            ResponseXmlParseError::Scraper(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for ResponseXmlParseError {}

impl ResponseXmlBackend {
    pub fn parse_tool_response(&self, xml: &str) -> Result<XmlToolCall, ResponseXmlParseError> {
        match self {
            ResponseXmlBackend::QuickXml => quick_xml::parse_fragment(xml),
            ResponseXmlBackend::ScraperHtml => scraper_html::parse_fragment(xml),
        }
    }
}

/// [`XmlToolParser`](crate::xml_tool_caller::XmlToolParser) entry: ScraperHtml, then relaxed `quick-xml`.
pub fn parse_tool_response_default_chain(xml: &str) -> Result<XmlToolCall, ResponseXmlParseError> {
    parse_tool_response_envelope_default_chain(xml).map(|e| e.primary)
}

/// Full envelope: `<sidecar_tools>` (optional) + root primary tool fields.
pub fn parse_tool_response_envelope_default_chain(
    xml: &str,
) -> Result<XmlToolEnvelope, ResponseXmlParseError> {
    match scraper_html::parse_fragment_envelope(xml) {
        Ok(env) => Ok(env),
        Err(e_html) => {
            let primary = match quick_xml::parse_fragment_with_relax(xml, true) {
                Ok(c) => c,
                Err(e_relaxed) => {
                    return Err(ResponseXmlParseError::Scraper(format!(
                        "ScraperHtml envelope: {e_html}; relaxed QuickXml: {e_relaxed}"
                    )));
                }
            };
            log::debug!(
                "response_xml: envelope ScraperHtml failed ({e_html}); using relaxed QuickXml primary-only (no sidecar)"
            );
            Ok(XmlToolEnvelope {
                sidecar: Vec::new(),
                primary,
            })
        }
    }
}

/// Convert XML string arguments to JSON for `parse_tool_call_arguments` / tool handlers.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn backends() -> [ResponseXmlBackend; 2] {
        [ResponseXmlBackend::QuickXml, ResponseXmlBackend::ScraperHtml]
    }

    fn file_edit_edits_array(call: &XmlToolCall) -> Value {
        let edits = call
            .arguments
            .get("edits")
            .expect("file:edit must include <edits> JSON array");
        serde_json::from_str(edits).expect("edits must be valid JSON")
    }

    fn file_edit_first_field(call: &XmlToolCall, key: &str) -> String {
        let arr = file_edit_edits_array(call);
        let first = arr.get(0).expect("edits[0]");
        first
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("edits[0].{key} missing or not a string: {first:?}"))
            .to_string()
    }

    #[test]
    fn both_parse_complete_response() {
        let xml = r#"<response>
            <thoughts>Handle user request</thoughts>
            <headline>Execute</headline>
            <tool_name>response</tool_name>
            <tool_args>
                <text>Hello</text>
            </tool_args>
        </response>"#;
        for b in backends() {
            let call = b
                .parse_tool_response(xml)
                .unwrap_or_else(|e| panic!("{b:?}: {e}"));
            assert_eq!(call.name, "response", "{b:?}");
            assert_eq!(call.thoughts.trim(), "Handle user request", "{b:?}");
            assert_eq!(call.headline.trim(), "Execute", "{b:?}");
            assert_eq!(call.arguments.get("text").map(String::as_str), Some("Hello"), "{b:?}");
        }
    }

    #[test]
    fn both_parse_missing_open_response_wrapper() {
        let xml = r#"<thoughts>t</thoughts>
<headline>h</headline>
<tool_name>wait</tool_name>
<tool_args><seconds>1</seconds></tool_args>
</response>"#;
        for b in backends() {
            let call = b.parse_tool_response(xml).unwrap_or_else(|e| panic!("{b:?}: {e}"));
            assert_eq!(call.name, "wait", "{b:?}");
            assert_eq!(call.arguments.get("seconds").map(String::as_str), Some("1"), "{b:?}");
        }
    }

    #[test]
    fn both_parse_multiple_args() {
        let xml = r#"<response>
            <tool_name>custom</tool_name>
            <tool_args>
                <arg1>value1</arg1>
                <arg2>value2</arg2>
            </tool_args>
        </response>"#;
        for b in backends() {
            let call = b.parse_tool_response(xml).unwrap_or_else(|e| panic!("{b:?}: {e}"));
            assert_eq!(call.arguments.get("arg1").map(String::as_str), Some("value1"), "{b:?}");
            assert_eq!(call.arguments.get("arg2").map(String::as_str), Some("value2"), "{b:?}");
        }
    }

    #[test]
    fn both_parse_minimal() {
        let xml = "<response><tool_name>test</tool_name></response>";
        for b in backends() {
            let call = b.parse_tool_response(xml).unwrap_or_else(|e| panic!("{b:?}: {e}"));
            assert_eq!(call.name, "test", "{b:?}");
            assert!(call.arguments.is_empty(), "{b:?}");
        }
    }

    #[test]
    fn both_parse_file_edit_cdata_embedded_markup() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <edits><![CDATA[[{"path":"src/App.vue","oldString":"  <div v-if=\"ok\">x</div>  ","newString":"  <div v-if=\"ok\">y</div>  "}]]]></edits>
  </tool_args>
</response>"#;
        for b in backends() {
            let call = b.parse_tool_response(xml).unwrap_or_else(|e| panic!("{b:?}: {e}"));
            assert_eq!(call.name, "file:edit", "{b:?}");
            assert!(file_edit_first_field(&call, "oldString").contains("v-if"), "{b:?} oldString");
            assert!(file_edit_first_field(&call, "newString").contains("v-if"), "{b:?} newString");
            assert_eq!(file_edit_first_field(&call, "path"), "src/App.vue");
        }
    }

    #[test]
    fn bare_ampersand_in_text_both() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <edits><![CDATA[[{"path":"x.txt","oldString":"foo & bar","newString":"ok"}]]]></edits>
  </tool_args>
</response>"#;
        for b in backends() {
            let call = b.parse_tool_response(xml).unwrap_or_else(|e| panic!("{b:?}: {e}"));
            assert_eq!(
                file_edit_first_field(&call, "oldString"),
                "foo & bar",
                "{b:?}"
            );
        }
    }

    #[test]
    fn literal_gt_in_text_both() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <edits><![CDATA[[{"path":"x.txt","oldString":"a > b","newString":"z"}]]]></edits>
  </tool_args>
</response>"#;
        for b in backends() {
            let call = b.parse_tool_response(xml).unwrap_or_else(|e| panic!("{b:?}: {e}"));
            assert_eq!(file_edit_first_field(&call, "oldString"), "a > b", "{b:?}");
        }
    }

    /// `&` inside JSON strings in CDATA is literal for both parsers (no HTML entity decoding on the JSON blob).
    #[test]
    fn ampersand_in_json_edits_cdata_same_for_both_backends() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <edits><![CDATA[[{"path":"x.txt","oldString":"foo & bar","newString":"z"}]]]></edits>
  </tool_args>
</response>"#;
        for b in backends() {
            let call = b.parse_tool_response(xml).unwrap_or_else(|e| panic!("{b:?}: {e}"));
            assert_eq!(file_edit_first_field(&call, "oldString"), "foo & bar", "{b:?}");
        }
    }

    /// Unescaped `<` inside JSON strings is safe when the whole payload lives in CDATA.
    #[test]
    fn unescaped_lt_in_json_edits_cdata_parsed_by_both_backends() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <edits><![CDATA[[{"path":"x.txt","oldString":"if a < b","newString":"z"}]]]></edits>
  </tool_args>
</response>"#;
        for b in backends() {
            let call = b.parse_tool_response(xml).unwrap_or_else(|e| panic!("{b:?}: {e}"));
            assert_eq!(file_edit_first_field(&call, "oldString"), "if a < b", "{b:?}");
        }
    }

    #[test]
    fn quick_xml_relaxed_end_tags_config_smoke() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <edits><![CDATA[[{"path":"x.txt","oldString":"if a < b","newString":"z"}]]]></edits>
  </tool_args>
</response>"#;
        assert!(quick_xml::parse_fragment_with_relax(xml, false).is_ok());
        let relaxed = quick_xml::parse_fragment_with_relax(xml, true).expect("relaxed");
        assert_eq!(
            file_edit_first_field(&relaxed, "oldString"),
            "if a < b"
        );
    }

    #[test]
    fn bare_ampersand_strict_vs_relaxed_quick_xml() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <edits><![CDATA[[{"path":"x.txt","oldString":"foo & bar","newString":"ok"}]]]></edits>
  </tool_args>
</response>"#;
        let strict = quick_xml::parse_fragment_with_relax(xml, false).expect("strict");
        let relaxed = quick_xml::parse_fragment_with_relax(xml, true).expect("relaxed");
        assert_eq!(
            file_edit_first_field(&strict, "oldString"),
            file_edit_first_field(&relaxed, "oldString")
        );
    }

    #[test]
    fn literal_gt_quick_relaxed() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <edits><![CDATA[[{"path":"x.txt","oldString":"a > b","newString":"z"}]]]></edits>
  </tool_args>
</response>"#;
        for relax in [false, true] {
            let call = quick_xml::parse_fragment_with_relax(xml, relax)
                .unwrap_or_else(|e| panic!("relax={relax}: {e:?}"));
            assert_eq!(file_edit_first_field(&call, "oldString"), "a > b");
        }
    }

    // --- ScraperHtml: `oldString` with nested markup (beyond CDATA) ---

    /// Markup inside the JSON string value is preserved (not split as HTML child nodes).
    #[test]
    fn scraper_edits_json_preserves_angle_brackets_in_old_string() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <edits><![CDATA[[{"path":"src/Foo.vue","oldString":"before<span class=\"x\">mid</span>after","newString":"<div/>"}]]]></edits>
  </tool_args>
</response>"#;
        let call = ResponseXmlBackend::ScraperHtml
            .parse_tool_response(xml)
            .expect("scraper parse");
        assert_eq!(call.name, "file:edit");
        let old = file_edit_first_field(&call, "oldString");
        assert!(
            old.contains("<span") && old.contains("</span>") && old.contains("mid"),
            "expected markup preserved in JSON string, got {old:?}"
        );
        assert_eq!(file_edit_first_field(&call, "newString"), "<div/>");
        assert_eq!(file_edit_first_field(&call, "path"), "src/Foo.vue");
    }

    /// CDATA (pre-escaped for HTML5) keeps full markup string in `oldString`, including nested tags.
    #[test]
    fn scraper_oldstring_cdata_deeply_nested_markup_round_trips() {
        let inner = serde_json::json!([{
            "path": "p",
            "oldString": "  <div a=\"1\"><span><b>x</b></span></div>  ",
            "newString": "n"
        }]);
        let xml = format!(
            r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <edits><![CDATA[{}]]></edits>
  </tool_args>
</response>"#,
            inner.to_string()
        );
        let call = ResponseXmlBackend::ScraperHtml
            .parse_tool_response(&xml)
            .expect("scraper parse");
        let old = file_edit_first_field(&call, "oldString");
        assert!(
            old.contains("<div") && old.contains("</div>") && old.contains("<span>"),
            "expected markup preserved, got {old:?}"
        );
        assert!(old.contains("x"), "text inside nested tags: {old:?}");
        assert_eq!(file_edit_first_field(&call, "path"), "p");
        assert_eq!(file_edit_first_field(&call, "newString"), "n");
    }

    /// Large strings in JSON `edits` round-trip without dropping fields.
    #[test]
    fn scraper_oldstring_cdata_then_other_args_complete() {
        let inner = serde_json::json!([{
            "path": "z.ts",
            "oldString": "<template><p id=\"a\">1</p><p>2</p></template>",
            "newString": "<template><p>ok</p></template>"
        }]);
        let xml = format!(
            r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <edits><![CDATA[{}]]></edits>
  </tool_args>
</response>"#,
            inner.to_string()
        );
        let call = ResponseXmlBackend::ScraperHtml
            .parse_tool_response(&xml)
            .expect("scraper parse");
        assert_eq!(call.arguments.len(), 1);
        let old = file_edit_first_field(&call, "oldString");
        assert!(old.contains("<template>") && old.contains("id=\"a\""));
        let new_s = file_edit_first_field(&call, "newString");
        assert!(new_s.contains("<template>") && new_s.contains("ok"));
    }

    #[test]
    fn envelope_default_chain_parses_sidecar_and_primary() {
        let xml = r#"<response>
  <thoughts>t</thoughts>
  <headline>h</headline>
  <sidecar_tools>
    <call>
      <tool_name>task_board:patch</tool_name>
      <tool_args>
        <method>patch</method>
        <items>[]</items>
      </tool_args>
    </call>
  </sidecar_tools>
  <tool_name>terminal</tool_name>
  <tool_args>
    <command>echo ok</command>
  </tool_args>
</response>"#;
        let env = parse_tool_response_envelope_default_chain(xml).expect("envelope");
        assert_eq!(env.sidecar.len(), 1, "sidecar calls");
        assert_eq!(env.sidecar[0].name.trim(), "task_board:patch");
        assert_eq!(env.primary.name.trim(), "terminal");
        assert_eq!(
            env.primary.arguments.get("command").map(String::as_str),
            Some("echo ok")
        );
    }
}
