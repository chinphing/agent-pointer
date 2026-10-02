# Log levels (`run_chat`)

English | [简体中文](../../zh-CN/developer/logging.md)

Default console filters are usually `info`. Prefer:

| Level | Use for |
|-------|---------|
| **info** | Run lifecycle (`dispatch` / `run_chat start` / run status), idle job push (`idle_job_push: flushing` / defer), balance gate, workspace **policy changes** (user clear / create sandbox), compression **applied** or failures, task-board **new_turn** / invalid drops, **LLM client TTFT** (`stream_chat: first_token_ms=…`), **LLM token summary** at run end (`cache_hit` / `cache_miss`) |
| **debug** | Per-turn noise: credential cache / API key inject, transcript begin, compression skip-under-budget, sandbox **reuse**, task-board **reuse_active**, pre-stream timing (`http_until_response_headers_ms` etc.; requires `internal_runtime_log`), per-round LLM token lines, **`openai_compat_request`** (flattened chat/completions body minus `messages` / `tools`) |
| **warn** | Recoverable errors that should not silent-fail |

### LLM streaming latency

- **`stream_chat: first_token_ms=`** (info, once per round): from the HTTP POST until the first non-empty `content` / `reasoning` / `tool_calls` delta. The same line carries `kind=` and `http_until_headers_ms=` (response-header time, to distinguish queueing from first packet).
- This is **client-side TTFT**, not the server-side TTFT of vLLM `/metrics`.

Do not log secrets (raw API keys, tokens, full OAuth payloads).
