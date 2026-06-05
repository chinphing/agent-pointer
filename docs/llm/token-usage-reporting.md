# Token usage reporting (per agent instance)

## Identity fields

| Field | Purpose |
|-------|---------|
| `platform_agent_id` | OAuth desktop binding (JWT `agent_id`); auth only |
| `agent_instance_id` | New UUID per lead / sub-agent / supervisor segment; stats + logs + dedup |
| `agent_role_id` | Template id (`coder`, `explore`, `supervisor`) for admin filters |

Runtime logs on LLM / sub-agent paths use `agent_instance_id` + `agent_role_id`, not the platform agent UUID.

## Client upload (pointer-app)

After each `run_chat`, `finalize_run` enqueues one pending row per agent instance with usage in `usage_accum`. On app startup or exit, `finalize_all_stale_accum` enqueues any interrupted accum (no history archive) before clearing, then `flush_pending_reports` sends pending rows.

- `POST /auth/partner/token-usage` as `multipart/form-data`
- `metadata`: JSON (tokens, `model_totals`, ids, `request_id`, `period_*`)
- Per-round `model_totals` keys match the chat/completions `model` field actually sent (stream `Finish` / `chat_once` output). Top-level `model_name` is the highest-token model in `model_totals` when finalizing the run.
- `history_archive`: zip (`conversation_snapshot.json` inside)

Snapshots exclude `system` messages, redact images to `[image:n]` / `[computer_screen]`, and include only messages tagged with the reporting `agent_instance_id` plus the preceding user turn.

## Server

- Token rows in DB; conversation text only on disk under `TOKEN_USAGE_HISTORY_DIR` (see API `.env.example`).
- Archive store failure does **not** block billing (multipart still returns success; check API logs for `history archive store failed`).
- Billing: `billed_tokens = ceil(raw_total × ratio)` from `llm_model_token_ratios` (Qwen 3.5 baseline = 1.0).
- Quota: `token_quota_exhausted` blocks all users (including those with their own API keys).
