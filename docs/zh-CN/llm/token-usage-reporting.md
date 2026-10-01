# Token usage reporting (per agent instance)

## Identity fields

| Field | Purpose |
|-------|---------|
| `platform_agent_id` | OAuth desktop binding (JWT `agent_id`); auth only |
| `run_id` | UUID per `run_chat`; scopes finalize to one run |
| `agent_instance_id` | New UUID per lead / sub-agent segment; stats + logs + dedup |
| `agent_role_id` | Template id (`coder`, `explore`) for admin filters |
| `request_id` | `run:{run_id}:{agent_instance_id}:{model_name}`; server dedup key |

Runtime logs on LLM / sub-agent paths use `run_id`, `agent_instance_id`, and `agent_role_id`, not the platform agent UUID.

## Client storage (pointer-app)

All usage lives in SQLite table `usage_accum`. Each row is keyed by `(run_id, agent_instance_id, model_name)` and tracks one model's token usage for one agent instance in one `run_chat`. When a run uses multiple models, there is one row per model with full token breakdown (`prompt_tokens`, `completion_tokens`, `thinking_tokens`, `cached_tokens`, `total_tokens`, `llm_rounds`).

`source` is sticky across rounds: if any round used a user provider (`source=user`), the row uploads as `user`.

`report_status` flow:

| Status | Meaning |
|--------|---------|
| `accumulating` | Tokens are being added during the run |
| `pending` | Run finished (or stale recovery); ready to upload |
| `sent` | Successfully reported to the platform |

During the run, `record_round` inserts or updates rows with `report_status = accumulating`.
Context-compression summary calls use the **same** `run_id` / `agent_instance_id` as the lead or sub-agent that triggered them (including background precompress). They do **not** mint a separate `precompress-*` run.

Background jobs (sub-agent / terminal) are tagged with the parent `run_id`. After each `run_chat`, if any non-terminal job still shares that `run_id`, `finalize_run` is **deferred** until the last such job finishes (cancelled / failed / completed all count). Immediate finalize still attaches optional conversation archive zip from the lead history; deferred finalize promotes rows without a history archive (billing does not require it). Then `flush_unsent_reports` uploads.

If there are no background jobs for the run, behavior is unchanged: `finalize_run(run_id, …)` then flush at lead end.

Access token is ~60 minutes (client treats it expired 5 minutes early). A long turn can outlive that window. Flush therefore calls `refresh_if_needed` **only when there is pending work and the session is not logged in** — short turns do not hit the token API. If refresh fails or there is no session, pending stays for retry.

The next `run_chat` also flushes leftover pending right after its start-of-turn refresh, so a previous skip does not wait until that next turn ends.

On app startup or exit, `finalize_all_stale_accum` promotes interrupted `accumulating` rows with usage to `pending` (no history archive), then `flush_unsent_reports` (alias `flush_pending_reports`) uploads pending rows and marks them `sent` on success. Failed uploads stay `pending` for retry.

## Client upload

- `POST /auth/partner/token-usage` as `multipart/form-data`
- `metadata`: JSON (`model_name`, per-model token fields, ids, `request_id`, `period_*`, `cached_tokens`, `source`)
- One upload per `(run_id, agent_instance_id, model_name)` row. `model_name` matches the chat/completions `model` field actually sent (stream `Finish` / `chat_once` output).
- `cached_tokens`: sum of context-cache hits (`prompt_tokens_details.cached_tokens`) for the row
- `source`: same as `ProviderConfig.source` — `platform` (billed) or `user` (record-only, `billed_yuan=0`)
- `history_archive`: zip (`conversation_snapshot.json` inside)

Snapshots exclude `system` messages, redact images to `[image:n]` / `[computer_screen]`, and include only messages tagged with the reporting `agent_instance_id` plus the preceding user turn.

## Billing (official)

| Case | Charge |
|------|--------|
| `source=platform` | Normal yuan billing from balance |
| `source=user` | No charge; row stored with `source=user` |
| Old clients omitting `source` | Server treats models **not** in the platform catalog/ratio table as `user` (record-only) |

`tokens_consumed` still increments for all reports (stats). Cache hits are stored for analytics and do not change the billed token formula by themselves.

## Client gate (platform mode)

LLM usage is **post-paid soft overdraft** on the server (`charge_llm_yuan` always deducts, may go negative). The client stops **new user turns** when balance is exhausted:

| When | Check |
|------|--------|
| User sends a message (`run_chat` start) | Live `GET /auth/partner/balance` via `ensure_llm_allowed` |
| Mid-turn tool / multi-model rounds | No re-check (same turn may still soft-overdraft) |
| Cloud agent open / oauth code | Same balance check |

When exhausted, the official desktop UI shows a composer banner and error card with a **去充值** button that opens `{POINTER_WEB_BASE}/profile/billing` (the official build injects its web base at build time). Unbound (non-official) builds default to no usage upload and no billing URL unless you set the env vars.

Login / token exchange / `GET /auth/partner/llm-credentials` are unchanged and do **not** perform this gate.

Fail-closed: balance API failure blocks starting the turn (does **not** skip charging for already-completed rounds).

**Standalone deployment:** these gates are no-ops. Self-hosted servers use local API keys / license and do not call the official balance API.

Quota: `token_quota_exhausted` (`balance <= 0`) blocks new turns.
