# Chat run errors (`StreamEvent::Error`)

`StreamEvent::Error` is emitted **once**, at the end of [`run_chat`](../../crates/pointer-core/src/chat_service/session.rs), when the run returns `Err`.

## Rules

1. Stream / tool / agent loops must **not** emit `StreamEvent::Error`.
2. On failure, return `chat_run_err(message, Some(assistant_message_id))` when a bubble should turn red; use a plain `anyhow::Error` (or `message_id: None`) when there is no target row.
3. Optional `MessageEnd` before returning is fine (close the streaming row); the session Error then attaches via `messageId`.

Helpers: `chat_service::emit::{chat_run_err, chat_run_error_parts}`.
