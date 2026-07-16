# Logging levels (run_chat)

Default console filters are usually `info`. Prefer:

| Level | Use for |
|-------|---------|
| **info** | Run lifecycle (`dispatch` / `run_chat start` / run status), balance gate, workspace **policy changes** (user clear / create sandbox), compression **applied** or failures, task-board **new_turn** / invalid drops |
| **debug** | Per-turn noise: credential cache / API key inject, transcript begin, compression skip-under-budget, sandbox **reuse**, task-board **reuse_active**, pre-stream timing (`*_ms=`) |
| **warn** | Recoverable errors that should not silent-fail |

Do not log secrets (raw API keys, tokens, full OAuth payloads).
