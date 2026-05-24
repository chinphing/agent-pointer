//! SearchAgent: research sub-agent inner `web_search` round (SSE + AGENT.md system).

mod messages;
mod stream_round;
mod system_prompt;

pub use stream_round::execute;
