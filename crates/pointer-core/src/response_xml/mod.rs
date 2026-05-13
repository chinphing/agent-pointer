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

    fn backends() -> [ResponseXmlBackend; 2] {
        [ResponseXmlBackend::QuickXml, ResponseXmlBackend::ScraperHtml]
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
    <path>src/App.vue</path>
    <oldString><![CDATA[  <div v-if="ok">x</div>  ]]></oldString>
    <newString><![CDATA[  <div v-if="ok">y</div>  ]]></newString>
  </tool_args>
</response>"#;
        for b in backends() {
            let call = b.parse_tool_response(xml).unwrap_or_else(|e| panic!("{b:?}: {e}"));
            assert_eq!(call.name, "file:edit", "{b:?}");
            assert_eq!(
                call.arguments.get("path").map(String::as_str),
                Some("src/App.vue"),
                "{b:?}"
            );
            assert!(
                call.arguments.get("oldString").unwrap().contains("v-if"),
                "{b:?} oldString"
            );
            assert!(
                call.arguments.get("newString").unwrap().contains("v-if"),
                "{b:?} newString"
            );
        }
    }

    #[test]
    fn bare_ampersand_in_text_both() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <path>x.txt</path>
    <oldString>foo & bar</oldString>
    <newString>ok</newString>
  </tool_args>
</response>"#;
        for b in backends() {
            let call = b.parse_tool_response(xml).unwrap_or_else(|e| panic!("{b:?}: {e}"));
            assert_eq!(
                call.arguments.get("oldString").map(String::as_str),
                Some("foo & bar"),
                "{b:?}"
            );
        }
    }

    #[test]
    fn literal_gt_in_text_both() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <path>x.txt</path>
    <oldString>a > b</oldString>
    <newString>z</newString>
  </tool_args>
</response>"#;
        for b in backends() {
            let call = b.parse_tool_response(xml).unwrap_or_else(|e| panic!("{b:?}: {e}"));
            assert_eq!(
                call.arguments.get("oldString").map(String::as_str),
                Some("a > b"),
                "{b:?}"
            );
        }
    }

    /// quick-xml keeps `&amp;` bytes in text; HTML5 decodes character references in text nodes.
    #[test]
    fn amp_entity_differs_by_backend() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <path>x.txt</path>
    <oldString>foo &amp; bar</oldString>
    <newString>z</newString>
  </tool_args>
</response>"#;
        let quick = ResponseXmlBackend::QuickXml
            .parse_tool_response(xml)
            .unwrap();
        assert_eq!(
            quick.arguments.get("oldString").map(String::as_str),
            Some("foo &amp; bar")
        );
        let html = ResponseXmlBackend::ScraperHtml
            .parse_tool_response(xml)
            .unwrap();
        assert_eq!(
            html.arguments.get("oldString").map(String::as_str),
            Some("foo & bar")
        );
    }

    /// quick-xml rejects unescaped `<` in text; HTML5 fragment parsing often keeps it inside
    /// unknown/custom elements — this is the main reason to offer [`ResponseXmlBackend::ScraperHtml`].
    #[test]
    fn unescaped_lt_quick_errors_scraper_preserves_text() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <path>x.txt</path>
    <oldString>if a < b</oldString>
    <newString>z</newString>
  </tool_args>
</response>"#;
        assert!(ResponseXmlBackend::QuickXml.parse_tool_response(xml).is_err());
        let s = ResponseXmlBackend::ScraperHtml
            .parse_tool_response(xml)
            .expect("scraper should produce a tree");
        assert_eq!(
            s.arguments.get("oldString").map(String::as_str),
            Some("if a < b")
        );
    }

    #[test]
    fn quick_xml_relaxed_end_tags_config_smoke() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <path>x.txt</path>
    <oldString>if a < b</oldString>
    <newString>z</newString>
  </tool_args>
</response>"#;
        assert!(quick_xml::parse_fragment_with_relax(xml, false).is_err());
        match quick_xml::parse_fragment_with_relax(xml, true) {
            Err(_) => {}
            Ok(call) => {
                assert_ne!(
                    call.arguments.get("oldString").map(String::as_str),
                    Some("if a < b")
                );
            }
        }
    }

    #[test]
    fn bare_ampersand_strict_vs_relaxed_quick_xml() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <path>x.txt</path>
    <oldString>foo & bar</oldString>
    <newString>ok</newString>
  </tool_args>
</response>"#;
        let strict = quick_xml::parse_fragment_with_relax(xml, false).expect("strict");
        let relaxed = quick_xml::parse_fragment_with_relax(xml, true).expect("relaxed");
        assert_eq!(
            strict.arguments.get("oldString"),
            relaxed.arguments.get("oldString")
        );
    }

    #[test]
    fn literal_gt_quick_relaxed() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <path>x.txt</path>
    <oldString>a > b</oldString>
    <newString>z</newString>
  </tool_args>
</response>"#;
        for relax in [false, true] {
            let call = quick_xml::parse_fragment_with_relax(xml, relax)
                .unwrap_or_else(|e| panic!("relax={relax}: {e:?}"));
            assert_eq!(
                call.arguments.get("oldString").map(String::as_str),
                Some("a > b")
            );
        }
    }

    // --- ScraperHtml: `oldString` with nested markup (beyond CDATA) ---

    /// Raw child tags under `<oldString>` are real HTML nodes; the scraper backend only joins
    /// descendant text nodes, so angle brackets are **not** preserved.
    #[test]
    fn scraper_oldstring_nested_tags_yields_concatenated_text_only() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <path>src/Foo.vue</path>
    <oldString>before<span class="x">mid</span>after</oldString>
    <newString><![CDATA[<div/>]]></newString>
  </tool_args>
</response>"#;
        let call = ResponseXmlBackend::ScraperHtml
            .parse_tool_response(xml)
            .expect("scraper parse");
        assert_eq!(call.name, "file:edit");
        assert_eq!(call.arguments.get("path").map(String::as_str), Some("src/Foo.vue"));
        assert_eq!(
            call.arguments.get("oldString").map(String::as_str),
            Some("beforemidafter")
        );
        assert_eq!(
            call.arguments.get("newString").map(String::as_str),
            Some("<div/>")
        );
    }

    /// CDATA (pre-escaped for HTML5) keeps full markup string in `oldString`, including nested tags.
    #[test]
    fn scraper_oldstring_cdata_deeply_nested_markup_round_trips() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <path>p</path>
    <oldString><![CDATA[  <div a="1"><span><b>x</b></span></div>  ]]></oldString>
    <newString>n</newString>
  </tool_args>
</response>"#;
        let call = ResponseXmlBackend::ScraperHtml
            .parse_tool_response(xml)
            .expect("scraper parse");
        let old = call.arguments.get("oldString").expect("oldString");
        assert!(
            old.contains("<div") && old.contains("</div>") && old.contains("<span>"),
            "expected markup preserved, got {old:?}"
        );
        assert!(old.contains("x"), "text inside nested tags: {old:?}");
        assert_eq!(call.arguments.get("path").map(String::as_str), Some("p"));
        assert_eq!(call.arguments.get("newString").map(String::as_str), Some("n"));
    }

    /// Another sibling arg after a large CDATA `oldString` must still parse (no swallowed `newString`).
    #[test]
    fn scraper_oldstring_cdata_then_other_args_complete() {
        let xml = r#"<response>
  <tool_name>file:edit</tool_name>
  <tool_args>
    <path>z.ts</path>
    <oldString><![CDATA[<template><p id="a">1</p><p>2</p></template>]]></oldString>
    <newString><![CDATA[<template><p>ok</p></template>]]></newString>
  </tool_args>
</response>"#;
        let call = ResponseXmlBackend::ScraperHtml
            .parse_tool_response(xml)
            .expect("scraper parse");
        assert_eq!(call.arguments.len(), 3);
        assert_eq!(call.arguments.get("path").map(String::as_str), Some("z.ts"));
        let old = call.arguments.get("oldString").unwrap();
        assert!(old.contains("<template>") && old.contains("id=\"a\""));
        let new_s = call.arguments.get("newString").unwrap();
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
