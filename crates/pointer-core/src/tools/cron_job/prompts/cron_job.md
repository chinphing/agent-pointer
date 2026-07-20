Manage **scheduled agent runs** (cron jobs). Each job fires on a timer and runs
your **prompt_text** in a dedicated cron session (separate from this chat).

WHEN TO USE

- The user asks to **remind**, **run once later** (in N minutes / at a time),
  **run regularly**, **every day at …**, or **schedule a recurring task**.
- Confirm the **prompt** (what the agent should do) and the **schedule**
  before calling create.

CREATE (minimal)

  cron_job(
    action="create",
    prompt_text="Check weather and post a one-line summary",
    schedule="daily@9:30"
  )

One-shot reminder (fires once, then soft-completes — row kept disabled for
history / session viewing):

  cron_job(
    action="create",
    prompt_text="Remind the user to stretch",
    schedule="30m"
  )

Only **prompt_text** and **schedule** are required. Optional **label** overrides
the auto-generated short name.

DELIVER (optional)

Pass **deliver** to push the run's final reply to IM after it finishes.
Omit / empty = no push (the cron session still records the transcript).

Use **channel names only** (one binding per channel — the last person who
privately messaged Pointer on that channel):

- `feishu` / `dingtalk` / `wecom` / `weixin`
- comma-separated for several, e.g. `feishu,dingtalk`
- `all` — every channel that already has a binding

If a channel is not bound yet, create fails — tell the user to send Pointer
a private message on that channel first, then retry.

If the run reply is just `[SILENT]`, no message is pushed.

SCHEDULE FORMAT

One-shot (local timezone; fires once then soft-completes):

- Relative: `30m`, `2h`, `1d`
- Absolute: `2026-07-22T09:00:00` (no offset = local)

Recurring presets (local timezone):

- `every_minute` or `每分钟`
- `every_5_minutes` or `每10分钟`
- `every_2_hours` or `每2小时`
- `daily@9:30` or `每天 09:30`
- `weekly@1@9:30` (0=Sunday … 6=Saturday) or `每周一 9:30`
- `monthly@1@9:00` or `每月1日 9:00`
- Raw 6-field cron: `0 30 9 * * *` (sec min hour dom mon dow)

Scheduler ticks about every 60 seconds — one-shot times may be up to ~1 minute late.

LIST

  cron_job(action="list")

ENABLE / DISABLE / DELETE

  cron_job(action="disable", job_id="cron-abc123def456")
  cron_job(action="enable", job_id="cron-abc123def456")
  cron_job(action="delete", job_id="cron-abc123def456")

Completed one-shot jobs cannot be re-enabled — create a new job instead.
Users can still open the cron session under **Settings → Automation**.

After create, tell the user the **schedule**, **next run time**, and that they
can also manage jobs under **Settings → Automation**.
