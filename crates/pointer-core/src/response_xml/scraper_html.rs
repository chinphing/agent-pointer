use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use super::{wrap_synthetic_response_root, ResponseXmlParseError, XmlToolCall};

fn cdata_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)<!\[CDATA\[(.*?)\]\]>").expect("CDATA regex"))
}

/// CDATA is not reliably preserved in HTML5 fragment parsing; pre-escape inner bytes so the tree
/// stores them as normal character data (browser-style entity decoding on read).
fn expand_cdata_for_html5(input: &str) -> String {
    cdata_regex()
        .replace_all(input, |caps: &regex::Captures<'_>| {
            let inner = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            inner
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
        })
        .into_owned()
}

/// HTML lowercases tag names; map back to tool-arg keys expected by XML tool handlers.
fn normalize_tool_arg_key(html_local_name: &str) -> String {
    match html_local_name {
        "oldstring" => "oldString".to_string(),
        "newstring" => "newString".to_string(),
        _ => html_local_name.to_string(),
    }
}

fn element_text_content(el: ElementRef<'_>) -> String {
    el.text().collect::<Vec<_>>().join("")
}

pub(super) fn parse_fragment(xml: &str) -> Result<XmlToolCall, ResponseXmlParseError> {
    let wrapped = wrap_synthetic_response_root(xml);
    let prepared = expand_cdata_for_html5(wrapped.as_ref());
    let doc = Html::parse_fragment(&prepared);

    let response_sel = Selector::parse("response")
        .map_err(|e| ResponseXmlParseError::Scraper(e.to_string()))?;
    let response = doc
        .select(&response_sel)
        .next()
        .ok_or_else(|| ResponseXmlParseError::Scraper("missing <response> element".into()))?;

    let mut call = XmlToolCall {
        name: String::new(),
        arguments: HashMap::new(),
        thoughts: String::new(),
        headline: String::new(),
    };

    for child in response.child_elements() {
        match child.value().name() {
            "thoughts" => call.thoughts = element_text_content(child),
            "headline" => call.headline = element_text_content(child),
            "tool_name" => call.name = element_text_content(child).trim().to_string(),
            "tool_args" => {
                for arg_el in child.child_elements() {
                    let key = normalize_tool_arg_key(arg_el.value().name());
                    let val = element_text_content(arg_el);
                    call.arguments.insert(key, val);
                }
            }
            _ => {}
        }
    }

    Ok(call)
}
