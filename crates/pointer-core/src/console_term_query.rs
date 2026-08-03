//! Answer terminal capability queries next to the console PTY.
//!
//! Workspace console uses a split architecture (Rust PTY ↔ frontend xterm.js).
//! If OSC/DA/CPR queries are forwarded to xterm, replies travel back through
//! async IPC and often land in the shell after Ctrl+C. Intercepting queries on
//! the PTY output path and answering locally removes that round trip.

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const LBRACKET: u8 = b'[';
const RBRACKET: u8 = b']';
const BACKSLASH: u8 = b'\\';
const HOLD_MAX: usize = 256;

/// Primary DA — VT100 with AVO (matches common xterm.js / libvte answers).
const DA1_REPLY: &[u8] = b"\x1b[?1;2c";
/// Secondary DA — xterm-style version stamp (276 ≈ xterm.js family).
const DA2_REPLY: &[u8] = b"\x1b[>0;276;0c";
/// Cursor position report stub (home). Accurate tracking needs a full emulator;
/// a fast local answer prevents Ctrl+C reply leaks into the shell.
const CPR_REPLY: &[u8] = b"\x1b[1;1R";

/// Default palette aligned with Pointer dark card / light foreground.
const DEFAULT_FG: &str = "rgb:f8f8/fafa/fcfc";
const DEFAULT_BG: &str = "rgb:0f0f/1515/2424";
const DEFAULT_CURSOR: &str = "rgb:8888/8888/ffff";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    AfterEsc,
    Csi,
    Osc,
    OscEsc, // ESC inside OSC — maybe ST (`ESC \`)
}

/// Stateful filter: strip capability queries from PTY→UI bytes and emit replies.
#[derive(Debug, Clone)]
pub struct TermQueryFilter {
    state: State,
    hold: Vec<u8>,
    fg: String,
    bg: String,
    cursor: String,
}

impl Default for TermQueryFilter {
    fn default() -> Self {
        Self::with_colors(DEFAULT_FG, DEFAULT_BG, DEFAULT_CURSOR)
    }
}

impl TermQueryFilter {
    pub fn with_colors(fg: &str, bg: &str, cursor: &str) -> Self {
        Self {
            state: State::Idle,
            hold: Vec::with_capacity(32),
            fg: fg.to_string(),
            bg: bg.to_string(),
            cursor: cursor.to_string(),
        }
    }

    /// Update reported OSC 10/11/12 colors (e.g. after UI theme change).
    pub fn set_colors(&mut self, fg: &str, bg: &str, cursor: &str) {
        self.fg = fg.to_string();
        self.bg = bg.to_string();
        self.cursor = cursor.to_string();
    }

    /// Process a PTY output chunk.
    /// - `forward`: bytes to decode and send to the UI
    /// - `replies`: capability answers to write back to the PTY immediately
    pub fn process(&mut self, input: &[u8], forward: &mut Vec<u8>, replies: &mut Vec<Vec<u8>>) {
        if matches!(self.state, State::Idle) && !input.contains(&ESC) {
            forward.extend_from_slice(input);
            return;
        }

        for &b in input {
            match self.state {
                State::Idle => {
                    if b == ESC {
                        self.state = State::AfterEsc;
                        self.hold.clear();
                        self.hold.push(b);
                    } else {
                        forward.push(b);
                    }
                }
                State::AfterEsc => {
                    if b == LBRACKET {
                        self.state = State::Csi;
                        self.hold.push(b);
                    } else if b == RBRACKET {
                        self.state = State::Osc;
                        self.hold.push(b);
                    } else if b == ESC {
                        forward.extend_from_slice(&self.hold);
                        self.hold.clear();
                        self.hold.push(b);
                    } else {
                        forward.extend_from_slice(&self.hold);
                        forward.push(b);
                        self.hold.clear();
                        self.state = State::Idle;
                    }
                }
                State::Csi => {
                    self.hold.push(b);
                    if (0x40..=0x7e).contains(&b) {
                        self.finish_csi(forward, replies);
                    } else if self.hold.len() >= HOLD_MAX {
                        forward.extend_from_slice(&self.hold);
                        self.hold.clear();
                        self.state = State::Idle;
                    }
                }
                State::Osc => {
                    self.hold.push(b);
                    if b == BEL {
                        self.finish_osc(forward, replies);
                    } else if b == ESC {
                        self.state = State::OscEsc;
                    } else if self.hold.len() >= HOLD_MAX {
                        forward.extend_from_slice(&self.hold);
                        self.hold.clear();
                        self.state = State::Idle;
                    }
                }
                State::OscEsc => {
                    self.hold.push(b);
                    if b == BACKSLASH {
                        self.finish_osc(forward, replies);
                    } else if b == ESC {
                        // ESC ESC — keep collecting OSC
                        self.state = State::Osc;
                    } else {
                        // Not ST; treat as OSC body continuing after a lone ESC
                        self.state = State::Osc;
                        if self.hold.len() >= HOLD_MAX {
                            forward.extend_from_slice(&self.hold);
                            self.hold.clear();
                            self.state = State::Idle;
                        }
                    }
                }
            }
        }
    }

    /// Flush any incomplete hold (EOF / session end).
    pub fn finish(&mut self, forward: &mut Vec<u8>) {
        if !self.hold.is_empty() {
            forward.extend_from_slice(&self.hold);
            self.hold.clear();
        }
        self.state = State::Idle;
    }

    fn finish_csi(&mut self, forward: &mut Vec<u8>, replies: &mut Vec<Vec<u8>>) {
        let final_byte = *self.hold.last().unwrap_or(&0);
        let middle = if self.hold.len() >= 3 {
            &self.hold[2..self.hold.len() - 1]
        } else {
            &[]
        };

        match final_byte {
            b'c' => {
                // Responses contain '?' or start with '>' and include ';version' —
                // queries are ESC[c, ESC[0c, ESC[>c, ESC[>0c (no '?' in query).
                if middle.contains(&b'?') {
                    forward.extend_from_slice(&self.hold);
                } else if middle.first() == Some(&b'>') {
                    // Secondary DA query (ESC[>c / ESC[>0c). A response looks like
                    // ESC[>0;276;0c — those contain ';' and must pass through.
                    if middle.contains(&b';') {
                        forward.extend_from_slice(&self.hold);
                    } else {
                        replies.push(DA2_REPLY.to_vec());
                    }
                } else if middle.first() == Some(&b'=') {
                    // Tertiary DA — ignore (no reply), swallow
                } else if middle.is_empty() || middle.iter().all(|c| c.is_ascii_digit()) {
                    replies.push(DA1_REPLY.to_vec());
                } else {
                    forward.extend_from_slice(&self.hold);
                }
            }
            b'n' if middle == b"6" => {
                replies.push(CPR_REPLY.to_vec());
            }
            _ => {
                forward.extend_from_slice(&self.hold);
            }
        }
        self.hold.clear();
        self.state = State::Idle;
    }

    fn finish_osc(&mut self, forward: &mut Vec<u8>, replies: &mut Vec<Vec<u8>>) {
        // hold: ESC ] <body> BEL  or  ESC ] <body> ESC \
        let body = osc_body(&self.hold);
        if let Some(ps) = parse_osc_color_query(body) {
            let color = match ps {
                10 => self.fg.as_str(),
                11 => self.bg.as_str(),
                12 => self.cursor.as_str(),
                _ => {
                    forward.extend_from_slice(&self.hold);
                    self.hold.clear();
                    self.state = State::Idle;
                    return;
                }
            };
            replies.push(osc_color_reply(ps, color));
        } else {
            forward.extend_from_slice(&self.hold);
        }
        self.hold.clear();
        self.state = State::Idle;
    }
}

fn osc_body(hold: &[u8]) -> &[u8] {
    // Skip ESC ]
    if hold.len() < 3 {
        return &[];
    }
    let start = 2;
    let end = if hold.ends_with(&[ESC, BACKSLASH]) {
        hold.len() - 2
    } else if hold.last() == Some(&BEL) {
        hold.len() - 1
    } else {
        hold.len()
    };
    if end < start {
        &[]
    } else {
        &hold[start..end]
    }
}

/// `10;?` / `11;?` / `12;?` (optional spaces ignored).
fn parse_osc_color_query(body: &[u8]) -> Option<u8> {
    let s = std::str::from_utf8(body).ok()?.trim();
    let (ps, rest) = s.split_once(';')?;
    if rest.trim() != "?" {
        return None;
    }
    match ps.trim() {
        "10" => Some(10),
        "11" => Some(11),
        "12" => Some(12),
        _ => None,
    }
}

fn osc_color_reply(ps: u8, color: &str) -> Vec<u8> {
    // ST-terminated reply (xterm.js uses ST for REPORT).
    format!("\x1b]{ps};{color}\x1b\\").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(filter: &mut TermQueryFilter, input: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
        let mut forward = Vec::new();
        let mut replies = Vec::new();
        filter.process(input, &mut forward, &mut replies);
        (forward, replies)
    }

    #[test]
    fn plain_text_passes() {
        let mut f = TermQueryFilter::default();
        let (out, replies) = run(&mut f, b"hello\n");
        assert_eq!(out, b"hello\n");
        assert!(replies.is_empty());
    }

    #[test]
    fn da1_swallowed_and_answered() {
        let mut f = TermQueryFilter::default();
        let (out, replies) = run(&mut f, b"\x1b[c");
        assert!(out.is_empty());
        assert_eq!(replies, vec![DA1_REPLY.to_vec()]);
    }

    #[test]
    fn da2_swallowed_and_answered() {
        let mut f = TermQueryFilter::default();
        let (out, replies) = run(&mut f, b"\x1b[>0c");
        assert!(out.is_empty());
        assert_eq!(replies, vec![DA2_REPLY.to_vec()]);
    }

    #[test]
    fn da_response_not_looped() {
        let mut f = TermQueryFilter::default();
        let (out, replies) = run(&mut f, DA2_REPLY);
        assert_eq!(out, DA2_REPLY);
        assert!(replies.is_empty());
    }

    #[test]
    fn cpr_answered() {
        let mut f = TermQueryFilter::default();
        let (out, replies) = run(&mut f, b"\x1b[6n");
        assert!(out.is_empty());
        assert_eq!(replies, vec![CPR_REPLY.to_vec()]);
    }

    #[test]
    fn osc11_query_answered() {
        let mut f = TermQueryFilter::default();
        let (out, replies) = run(&mut f, b"\x1b]11;?\x07");
        assert!(out.is_empty());
        assert_eq!(replies.len(), 1);
        let reply = String::from_utf8(replies[0].clone()).unwrap();
        assert!(reply.starts_with("\x1b]11;rgb:"));
        assert!(reply.ends_with("\x1b\\"));
    }

    #[test]
    fn osc10_st_terminator() {
        let mut f = TermQueryFilter::default();
        let (out, replies) = run(&mut f, b"\x1b]10;?\x1b\\");
        assert!(out.is_empty());
        assert_eq!(replies.len(), 1);
        assert!(replies[0].starts_with(b"\x1b]10;"));
    }

    #[test]
    fn osc_set_passes_through() {
        let mut f = TermQueryFilter::default();
        let seq = b"\x1b]11;#112233\x07";
        let (out, replies) = run(&mut f, seq);
        assert_eq!(out, seq);
        assert!(replies.is_empty());
    }

    #[test]
    fn embedded_query_keeps_neighbors() {
        let mut f = TermQueryFilter::default();
        let (out, replies) = run(&mut f, b"pre\x1b[0cpost");
        assert_eq!(out, b"prepost");
        assert_eq!(replies, vec![DA1_REPLY.to_vec()]);
    }

    #[test]
    fn split_osc_across_chunks() {
        let mut f = TermQueryFilter::default();
        let (o1, r1) = run(&mut f, b"\x1b]11;");
        assert!(o1.is_empty() && r1.is_empty());
        let (o2, r2) = run(&mut f, b"?\x07");
        assert!(o2.is_empty());
        assert_eq!(r2.len(), 1);
    }

    #[test]
    fn color_probe_bundle() {
        // Typical shell/CLI probe: OSC10 + OSC11 + DA2 + CPR
        let mut f = TermQueryFilter::default();
        let bundle = b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b[>0c\x1b[6n";
        let (out, replies) = run(&mut f, bundle);
        assert!(out.is_empty(), "queries must not reach the UI");
        assert_eq!(replies.len(), 4);
    }
}
