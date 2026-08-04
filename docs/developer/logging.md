# Logging levels (run_chat)

Default console filters are usually `info`. Prefer:

| Level | Use for |
|-------|---------|
| **info** | Run lifecycle (`dispatch` / `run_chat start` / run status), balance gate, workspace **policy changes** (user clear / create sandbox), compression **applied** or failures, task-board **new_turn** / invalid drops, **LLM client TTFT** (`stream_chat: first_token_ms=…`), **LLM token summary** at run end (`cache_hit` / `cache_miss`) |
| **debug** | Per-turn noise: credential cache / API key inject, transcript begin, compression skip-under-budget, sandbox **reuse**, task-board **reuse_active**, pre-stream timing (`http_until_response_headers_ms` 等，需 `internal_runtime_log`), per-round LLM token lines |
| **warn** | Recoverable errors that should not silent-fail |

### LLM 流式延迟

- **`stream_chat: first_token_ms=`**（info，每轮一次）：从 HTTP POST 开始到首个非空 `content` / `reasoning` / `tool_calls` delta。同条含 `kind=` 与 `http_until_headers_ms=`（响应头耗时，便于区分排队 vs 首包）。
- 这是 **客户端 TTFT**，不是 vLLM `/metrics` 的服务端 TTFT。

Do not log secrets (raw API keys, tokens, full OAuth payloads).
