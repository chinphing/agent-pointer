use std::collections::HashMap;

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use super::{wrap_synthetic_response_root, ResponseXmlParseError, XmlToolCall};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextSlot {
    Idle,
    Thoughts,
    Headline,
    ToolName,
    ToolArgs,
    Arg,
}

pub(super) fn parse_fragment(xml: &str) -> Result<XmlToolCall, ResponseXmlParseError> {
    parse_fragment_with_relax(xml, false)
}

/// For tests and experiments; production uses [`parse_fragment`] (`relax = false`).
pub(crate) fn parse_fragment_with_relax(
    xml: &str,
    relax_end_tag_checks: bool,
) -> Result<XmlToolCall, ResponseXmlParseError> {
    let wrapped = wrap_synthetic_response_root(xml);
    let mut reader = Reader::from_str(wrapped.as_ref());
    {
        let cfg = reader.config_mut();
        cfg.trim_text(true);
        cfg.expand_empty_elements = true;
        if relax_end_tag_checks {
            cfg.check_end_names = false;
            cfg.allow_unmatched_ends = true;
        }
    }

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
        let ev = reader
            .read_event()
            .map_err(|e| ResponseXmlParseError::QuickXml(e.to_string()))?;
        match ev {
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
