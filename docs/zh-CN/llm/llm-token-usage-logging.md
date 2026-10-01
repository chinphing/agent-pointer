# LLM token usage logging (pointer-core)

During each `run_chat` session, pointer-core accumulates OpenAI-compatible `usage` from chat/completions and prints logs to the process console (stderr when using flexi_logger / env_logger).

## Per-round (debug)

After each streaming or non-streaming model call in the main loop or sub-agents:

- `log::debug!` in `llm_token_stats` with round index and token fields when `usage` is present (`total`, `prompt`, `completion`, `cache_hit`, `cache_miss`).
- Missing `usage` for that round increments `rounds_missing_usage` (visible in the end-of-session summary).

Enable with a filter that includes debug for the module, for example:

`RUST_LOG=pointer_core::llm_token_stats=debug,pointer_core::provider=debug,pointer_core=info`

Streaming requests set `stream_options: { "include_usage": true }` by default (OpenAI-compatible). If a gateway returns HTTP errors for this field, set:

`POINTER_STREAM_INCLUDE_USAGE=0`

## End-of-session summary (info)

When the `ChatLlmTokenSession` guard is dropped at the end of `run_chat_inner`, an `log::info!` line is emitted (if there was at least one LLM round or one tool invocation) with:

- `conversation_id`
- `llm_rounds` — model calls (stream rounds + sub-agent stream rounds)
- `total_tokens`, `prompt_tokens` (summed over rounds)
- `cache_hit` / `cache_miss` — summed context-cache hit and miss prompt tokens (see below)
- `tool_invocations` — each tool run after validation (success or failure)

### Context cache fields

Parsed from OpenAI-compatible `usage`:

| Source | Field |
|--------|--------|
| Preferred | `usage.prompt_tokens_details.cached_tokens` |
| Fallback | top-level `usage.cached_tokens` (legacy DashScope) |

- **cache_hit** = reported `cached_tokens` (clamped to that round’s `prompt_tokens`)
- **cache_miss** = `prompt_tokens - cache_hit` for that round

Providers that omit cache details report `cache_hit=0` and `cache_miss=prompt_tokens`.

Context-compression `chat_once` calls do not add to this session accumulator.
