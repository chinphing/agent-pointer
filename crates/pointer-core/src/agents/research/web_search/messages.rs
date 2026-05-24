//! DashScope messages for ResearchSubAgent web_search (system + history + query).

use crate::agents::research::web_search::system_prompt::research_agent_md_body;
use crate::models::{ChatMessage, Role};
use crate::tools::web_search::WebSearchMessage;

/// Build messages: AGENT.md system, conversation history, final user query.
pub fn build_research_sub_agent_messages(
    history: &[ChatMessage],
    exclude_message_id: &str,
    query: &str,
) -> Vec<WebSearchMessage> {
    let mut messages = Vec::new();
    let system = research_agent_md_body();
    if !system.is_empty() {
        messages.push(WebSearchMessage {
            role: "system".into(),
            content: system.to_string(),
        });
    }
    for msg in history {
        if msg.id == exclude_message_id {
            continue;
        }
        match msg.role {
            Role::System => {
                let text = msg.content.trim();
                if !text.is_empty() {
                    messages.push(WebSearchMessage {
                        role: "system".into(),
                        content: text.to_string(),
                    });
                }
            }
            Role::User => {
                let text = msg.content.trim();
                if !text.is_empty() {
                    messages.push(WebSearchMessage {
                        role: "user".into(),
                        content: text.to_string(),
                    });
                }
            }
            Role::Assistant => {
                let mut parts = Vec::new();
                if !msg.content.trim().is_empty() {
                    parts.push(msg.content.trim().to_string());
                } else if let Some(thoughts) = msg.thoughts.as_deref().filter(|s| !s.trim().is_empty())
                {
                    parts.push(thoughts.trim().to_string());
                }
                if !parts.is_empty() {
                    messages.push(WebSearchMessage {
                        role: "assistant".into(),
                        content: parts.join("\n\n"),
                    });
                }
            }
            Role::Tool => {
                let text = msg.content.trim();
                if !text.is_empty() {
                    messages.push(WebSearchMessage {
                        role: "user".into(),
                        content: format!("[Tool result]\n{text}"),
                    });
                }
            }
        }
    }
    let q = query.trim();
    if !q.is_empty() {
        messages.push(WebSearchMessage {
            role: "user".into(),
            content: q.to_string(),
        });
    }
    messages
}
