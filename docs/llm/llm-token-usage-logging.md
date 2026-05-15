# LLM token usage logging (pointer-core)

During each `run_chat` session, pointer-core accumulates OpenAI-compatible `usage` from chat/completions and prints logs to the process console (stderr when using flexi_logger / env_logger).

## Per-round (debug)

After each streaming or non-streaming model call in the main loop, sub-agents, supervisor planning, or supervisor synthesis:

- `log::debug!` in `llm_token_stats` with round index and token fields when `usage` is present.
- `log::debug!` when `usage` is missing for that round (`rounds_missing_usage` in the summary).

Enable with a filter that includes debug for the module, for example:

`RUST_LOG=pointer_core::llm_token_stats=debug,pointer_core::provider=debug,pointer_core=info`

Streaming requests set `stream_options: { "include_usage": true }` by default (OpenAI-compatible). If a gateway returns HTTP errors for this field, set:

`POINTER_STREAM_INCLUDE_USAGE=0`

## End-of-session summary (info)

When the `ChatLlmTokenSession` guard is dropped at the end of `run_chat_inner`, an `log::info!` line is emitted (if there was at least one LLM round or one tool invocation) with:

- `conversation_id`
- `tool_invocations` — each non-`response` tool run after validation (success or failure)
- `llm_rounds` — model calls (stream rounds + supervisor plan + synthesize + sub-agent stream rounds)
- `total_tokens`, `prompt_tokens`, `reasoning_tokens`, `output_tokens` (completion minus reasoning, summed over rounds)
- `avg_tokens_per_tool` — `total_tokens / tool_invocations` when `tool_invocations > 0`
- `rounds_missing_usage` — stream/non-stream rounds with no `usage` payload

Context-compression `chat_once` calls do not add to this session accumulator.
