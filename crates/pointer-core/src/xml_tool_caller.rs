/// XML tool-call parsing: buffer until a full `<response>…</response>`, then parse via
/// [`crate::response_xml::parse_tool_response_envelope_default_chain`] (ScraperHtml → relaxed quick-xml).

pub use crate::response_xml::{
    parse_tool_response_default_chain, parse_tool_response_envelope_default_chain,
    xml_tool_arguments_to_json_string, ResponseXmlBackend, ResponseXmlParseError, XmlToolCall,
    XmlToolEnvelope,
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

/// Which API stream a chunk came from (feeds are merged in arrival order for XML parsing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlFeedLane {
    Content,
    Reasoning,
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
    /// 跳过「空壳」`<response>` 片段的次数（reasoning 先输出占位时常见）。
    pub vacuous_fragments_skipped: u32,
    /// 每次 `feed_lane` 的来源，`C`=content，`R`=reasoning（保留尾部若干次）。
    pub feed_lane_tail: String,
    /// 最终被解析消费的那一段 `<response>…</response>` 字符数（不含跳过的空壳）。
    pub consumed_fragment_chars: Option<usize>,
    /// 最终被消费片段的开头（单行压缩、截断，便于对照日志）。
    pub consumed_fragment_head: Option<String>,
    /// 流结束后解析器内部缓冲剩余字符数（正常完成时多为 0）。
    pub parser_buffer_remaining_chars: usize,
    /// 流式路径未得到工具调用时，已用「推理 + 正文」与界面相同顺序再 parse 并成功。
    pub merge_ui_order_reparse_ok: bool,
}

/// Streaming-safe parser: buffers until `</response>`, then parses the fragment.
pub struct XmlToolParser {
    buffer: String,
    is_complete: bool,
    current_envelope: Option<XmlToolEnvelope>,
    last_parse_error: Option<String>,
    fallback_thoughts: Option<String>,
    fallback_headline: Option<String>,
    pub(crate) ingest_content_chunks: u32,
    pub(crate) ingest_reasoning_chunks: u32,
    pub(crate) ingest_content_chars: usize,
    pub(crate) ingest_reasoning_chars: usize,
    pub(crate) feed_lane_tail: String,
    pub(crate) vacuous_fragments_skipped_total: u32,
    pub(crate) last_consumed_fragment_chars: Option<usize>,
    pub(crate) last_consumed_fragment_head: Option<String>,
}

fn compact_fragment_head(s: &str, max_chars: usize) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    let t = t.trim();
    let count = t.chars().count();
    if count <= max_chars {
        t.to_string()
    } else {
        format!(
            "{}…(+{} chars)",
            t.chars().take(max_chars).collect::<String>(),
            count.saturating_sub(max_chars)
        )
    }
}

impl XmlToolParser {
    pub(crate) fn unparsed_buffer_len(&self) -> usize {
        self.buffer.len()
    }

    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            is_complete: false,
            current_envelope: None,
            last_parse_error: None,
            fallback_thoughts: None,
            fallback_headline: None,
            ingest_content_chunks: 0,
            ingest_reasoning_chunks: 0,
            ingest_content_chars: 0,
            ingest_reasoning_chars: 0,
            feed_lane_tail: String::new(),
            vacuous_fragments_skipped_total: 0,
            last_consumed_fragment_chars: None,
            last_consumed_fragment_head: None,
        }
    }

    fn push_lane_tail(&mut self, lane: XmlFeedLane) {
        self.feed_lane_tail.push(match lane {
            XmlFeedLane::Content => 'C',
            XmlFeedLane::Reasoning => 'R',
        });
        const MAX: usize = 96;
        if self.feed_lane_tail.len() > MAX {
            let excess = self.feed_lane_tail.len() - MAX;
            self.feed_lane_tail.drain(..excess);
        }
    }

    fn record_consumed_fragment(&mut self, frag: &str) {
        self.last_consumed_fragment_chars = Some(frag.chars().count());
        self.last_consumed_fragment_head = Some(compact_fragment_head(frag, 260));
    }

    /// When [`Self::parse`] returns `None` but a `</response>` fragment existed, these may hold
    /// [`loose_extract_first_tag_inner`] results for UI / persistence.
    pub fn take_fallback_thoughts_headline(&mut self) -> (Option<String>, Option<String>) {
        (
            self.fallback_thoughts.take(),
            self.fallback_headline.take(),
        )
    }

    /// 与旧测试兼容：视为来自 **content** 流。
    pub fn feed(&mut self, chunk: &str) {
        self.feed_lane(chunk, XmlFeedLane::Content);
    }

    pub fn feed_lane(&mut self, chunk: &str, lane: XmlFeedLane) {
        if chunk.is_empty() {
            return;
        }

        match lane {
            XmlFeedLane::Content => {
                self.ingest_content_chunks += 1;
                self.ingest_content_chars += chunk.chars().count();
            }
            XmlFeedLane::Reasoning => {
                self.ingest_reasoning_chunks += 1;
                self.ingest_reasoning_chars += chunk.chars().count();
            }
        }
        self.push_lane_tail(lane);
        self.buffer.push_str(chunk);
        self.try_finish();
    }

    pub fn is_complete(&self) -> bool {
        self.is_complete
    }

    pub fn parse(&mut self) -> Option<XmlToolEnvelope> {
        if !self.is_complete {
            return None;
        }

        let env = self.current_envelope.take();
        self.is_complete = false;
        env
    }

    pub fn last_parse_error(&self) -> Option<&str> {
        self.last_parse_error.as_deref()
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
        self.current_envelope = None;
        self.is_complete = false;
        self.last_parse_error = None;
        self.fallback_thoughts = None;
        self.fallback_headline = None;
        self.ingest_content_chunks = 0;
        self.ingest_reasoning_chunks = 0;
        self.ingest_content_chars = 0;
        self.ingest_reasoning_chars = 0;
        self.feed_lane_tail.clear();
        self.vacuous_fragments_skipped_total = 0;
        self.last_consumed_fragment_chars = None;
        self.last_consumed_fragment_head = None;
    }

    fn try_finish(&mut self) {
        if self.is_complete {
            return;
        }
        /// 首个 `<response>…</response>` 无工具内容（常见于 reasoning 里先打 `<response></response>` 占位，正文里才是完整块）。
        fn is_vacuous_tool_xml(env: &XmlToolEnvelope) -> bool {
            env.sidecar.is_empty()
                && env.primary.name.trim().is_empty()
                && env.primary.thoughts.trim().is_empty()
                && env.primary.headline.trim().is_empty()
                && env.primary.arguments.is_empty()
        }

        const MAX_VACUOUS_SKIPS: usize = 32;
        let mut vacuous_skips: usize = 0;

        loop {
            let Some((end, frag)) = extract_response_fragment(&self.buffer) else {
                return;
            };
            match parse_tool_response_envelope_default_chain(&frag) {
                Ok(env) => {
                    let vacuous = is_vacuous_tool_xml(&env);
                    if vacuous && vacuous_skips < MAX_VACUOUS_SKIPS {
                        vacuous_skips += 1;
                        self.vacuous_fragments_skipped_total += 1;
                        self.buffer.drain(..end);
                        continue;
                    }
                    self.last_parse_error = None;
                    self.current_envelope = Some(env);
                    self.fallback_thoughts = None;
                    self.fallback_headline = None;
                    self.record_consumed_fragment(&frag);
                    self.is_complete = true;
                    self.buffer.drain(..end);
                    return;
                }
                Err(e) => {
                    self.last_parse_error = Some(e.to_string());
                    self.current_envelope = None;
                    self.fallback_thoughts = loose_extract_first_tag_inner(&frag, "thoughts");
                    self.fallback_headline = loose_extract_first_tag_inner(&frag, "headline");
                    self.record_consumed_fragment(&frag);
                    self.is_complete = true;
                    self.buffer.drain(..end);
                    return;
                }
            }
        }
    }
}

/// 流式阶段：在尚未出现完整 `</response>` 时，已从 `delta.content` 中可识别的闭合子标签（供前端渐进展示）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlStreamingPartial {
    pub thoughts: Option<String>,
    pub headline: Option<String>,
    pub tool_name: Option<String>,
}

/// 仅从 **正文 content** 缓冲提取当前可见的 `<thoughts>` / `<headline>` / `<tool_name>`（需开闭标签齐全）。
pub fn extract_xml_streaming_partial(buf: &str) -> XmlStreamingPartial {
    let thoughts = loose_extract_first_tag_inner(buf, "thoughts")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let headline = loose_extract_first_tag_inner(buf, "headline")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let tool_name = loose_extract_first_tag_inner(buf, "tool_name")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    XmlStreamingPartial {
        thoughts,
        headline,
        tool_name,
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

        let call = parser.parse().unwrap().primary;
        assert_eq!(call.name, "response");
        assert_eq!(call.thoughts.trim(), "Handle user request");
        assert_eq!(call.headline.trim(), "Execute");
        assert_eq!(call.arguments.get("text").unwrap(), "Hello");
    }

    /// Reasoning 与正文合并缓冲时，若先出现空的 `<response></response>`，再出现真实工具块，应解析后者。
    #[test]
    fn test_skip_leading_empty_response_then_parse_real_block() {
        let mut parser = XmlToolParser::new();
        parser.feed("<response></response>");
        assert!(!parser.is_complete());
        parser.feed(
            r#"<response>
  <thoughts>t</thoughts>
  <headline>h</headline>
  <tool_name>mouse:click_index</tool_name>
  <tool_args>
    <goal>g</goal>
    <action>a</action>
    <index>99</index>
  </tool_args>
</response>"#,
        );
        assert!(parser.is_complete());
        let call = parser.parse().expect("second block should win").primary;
        assert_eq!(call.name, "mouse:click_index");
        assert_eq!(call.arguments.get("index").map(String::as_str), Some("99"));
    }

    /// Opening tag is not literal `<response>` → synthetic outer wrapper → nested `<response>`;
    /// ScraperHtml must unwrap so `tool_name` / `tool_args` are visible.
    #[test]
    fn test_parse_response_root_with_attribute_nested_unwrap() {
        let xml = r#"<response foo="1">
  <thoughts>t</thoughts>
  <headline>h</headline>
  <tool_name>wait</tool_name>
  <tool_args><seconds>1</seconds></tool_args>
</response>"#;
        let mut parser = XmlToolParser::new();
        parser.feed(xml);
        assert!(parser.is_complete());
        let call = parser.parse().expect("nested response should unwrap").primary;
        assert_eq!(call.name, "wait");
        assert_eq!(call.thoughts.trim(), "t");
        assert_eq!(call.arguments.get("seconds").map(String::as_str), Some("1"));
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
        let call = parser.parse().unwrap().primary;
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

        let call = parser.parse().unwrap().primary;
        assert_eq!(call.name, "wait");
        assert_eq!(call.arguments.get("seconds").unwrap(), "5");
    }

    #[test]
    fn test_parse_minimal_xml() {
        let mut parser = XmlToolParser::new();
        parser.feed("<response><tool_name>test</tool_name></response>");

        let call = parser.parse().unwrap().primary;
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
        let call = parser.parse().unwrap().primary;
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
        let call = parser.parse().unwrap().primary;
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
        let call = parser.parse().unwrap().primary;
        assert_eq!(call.name, "test2");
    }

    #[test]
    fn test_parse_single_chunk_full_response() {
        let mut parser = XmlToolParser::new();
        let xml = "<response><tool_name>wait</tool_name><tool_args><seconds>5</seconds></tool_args></response>";
        parser.feed(xml);
        assert!(parser.is_complete());
        let call = parser.parse().unwrap().primary;
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
        let call = parser.parse().expect("ScraperHtml should parse this fragment").primary;
        assert_eq!(call.thoughts.trim(), "Planning edit");
        assert_eq!(call.headline.trim(), "Patch file");
        assert_eq!(call.name, "file:edit");
        assert_eq!(call.arguments.get("oldString").map(String::as_str), Some("if a < b"));
        assert_eq!(call.arguments.get("newString").map(String::as_str), Some("z"));
    }
}
