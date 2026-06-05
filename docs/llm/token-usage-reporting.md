# Token usage reporting (per agent instance)

## Identity fields

| Field | Purpose |
|-------|---------|
| `platform_agent_id` | OAuth desktop binding (JWT `agent_id`); auth only |
| `run_id` | UUID per `run_chat`; scopes finalize to one run |
| `agent_instance_id` | New UUID per lead / sub-agent / supervisor segment; stats + logs + dedup |
| `agent_role_id` | Template id (`coder`, `explore`, `supervisor`) for admin filters |
| `request_id` | `run:{run_id}:{agent_instance_id}:{model_name}`; server dedup key |

Runtime logs on LLM / sub-agent paths use `run_id`, `agent_instance_id`, and `agent_role_id`, not the platform agent UUID.

## Client storage (pointer-app)

All usage lives in SQLite table `usage_accum`. Each row is keyed by `(run_id, agent_instance_id, model_name)` and tracks one model's token usage for one agent instance in one `run_chat`. When a run uses multiple models, there is one row per model with full token breakdown (`prompt_tokens`, `completion_tokens`, `thinking_tokens`, `total_tokens`, `llm_rounds`).

`report_status` flow:

| Status | Meaning |
|--------|---------|
| `accumulating` | Tokens are being added during the run |
| `pending` | Run finished (or stale recovery); ready to upload |
| `sent` | Successfully reported to the platform |

During the run, `record_round` inserts or updates rows with `report_status = accumulating`.

After each `run_chat`, `finalize_run(run_id, …)` sets `report_status = pending` for that run's rows with usage (optional conversation archive zip). Token counts are **not** cleared.

On app startup or exit, `finalize_all_stale_accum` promotes interrupted `accumulating` rows with usage to `pending` (no history archive), then `flush_unsent_reports` (alias `flush_pending_reports`) uploads pending rows and marks them `sent` on success. Failed uploads stay `pending` for retry.

## Client upload

- `POST /auth/partner/token-usage` as `multipart/form-data`
- `metadata`: JSON (`model_name`, per-model token fields, ids, `request_id`, `period_*`)
- One upload per `(run_id, agent_instance_id, model_name)` row. `model_name` matches the chat/completions `model` field actually sent (stream `Finish` / `chat_once` output).
- `history_archive`: zip (`conversation_snapshot.json` inside)

Snapshots exclude `system` messages, redact images to `[image:n]` / `[computer_screen]`, and include only messages tagged with the reporting `agent_instance_id` plus the preceding user turn.

## Server

- Token rows in DB; conversation text only on disk under `TOKEN_USAGE_HISTORY_DIR` (see API `.env.example`).
- Archive store failure does **not** block billing (multipart still returns success; check API logs for `history archive store failed`).
- Billing: `billed_tokens = ceil(raw_total × ratio)` from `llm_model_token_ratios` (Qwen 3.5 baseline = 1.0).
- Quota: `token_quota_exhausted` blocks all users (including those with their own API keys).
