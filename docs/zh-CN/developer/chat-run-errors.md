# Chat run errors (`StreamEvent::Error`)

`StreamEvent::Error` is emitted **once**, at the end of [`run_chat`](../../../crates/pointer-core/src/chat_service/session.rs), when the run returns `Err`.

## Rules

1. Stream / tool / agent loops must **not** emit `StreamEvent::Error`.
2. On failure, return `chat_run_err(message, Some(assistant_message_id))` when a bubble should turn red; use a plain `anyhow::Error` (or `message_id: None`) when there is no target row.
3. Optional `MessageEnd` before returning is fine (close the streaming row); the session Error then attaches via `messageId`.
4. Tool-round cap: compress if enabled, then **one** `Error` on the assistant row.
   Do not also inject a 【提示】 bubble (`ToolRoundsExhausted` is ignored by the UI).

Helpers: `chat_service::emit::{chat_run_err, chat_run_error_parts}`.

## Panic containment

A panic is **not** a run failure path — if one escapes, the run never writes a terminal
status: `runs.status` stays `running`, no `RunFinished` / `RunFailed` / `Done` is emitted,
and every waiter (desktop stream, web stream, IM reply, `RunDispatcher::wait`) hangs with
nothing to show. Two layers keep that from happening:

1. **Tool handlers** — `ToolRegistry::invoke` wraps every handler in `catch_unwind` and
   returns the panic as a normal tool error, so one bad tool call cannot kill the run.
2. **Run boundary** — every caller that awaits `run_chat` wraps it in `catch_unwind` and
   maps the payload to `Err`, so the terminal status is always written. Current callers:
   `dispatcher::RunDispatcher::run_runner` and the IM channel dispatch in
   `pointer-channels`. **Any new `run_chat` call site must do the same.**

Use `logging::panic_payload_message` to render the payload. The global panic hook still
logs the message plus backtrace at `error` level, so the log keeps the real cause.

Common source of such panics: byte-index string slicing on multibyte text — see
[Rust 字符串截断](rust-text-truncation.md).
