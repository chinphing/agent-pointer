Manage **scheduled agent runs** (cron jobs). Each job fires on a timer and runs
your **prompt_text** in a dedicated cron session (separate from this chat).

WHEN TO USE

- The user asks to **remind**, **run something regularly**, **every day at …**,
  **every N minutes**, or **schedule a recurring task**.
- Confirm the **prompt** (what the agent should do each time) and the **schedule**
  before calling create.

CREATE (minimal)

  cron_job(
    action="create",
    prompt_text="Check weather and post a one-line summary",
    schedule="daily@9:30"
  )

Only **prompt_text** and **schedule** are required. Optional **label** overrides
the auto-generated short name.

SCHEDULE FORMAT

Friendly presets (local timezone):

- `every_minute` or `每分钟`
- `every_5_minutes` or `每10分钟`
- `every_2_hours` or `每2小时`
- `daily@9:30` or `每天 09:30`
- `weekly@1@9:30` (0=Sunday … 6=Saturday) or `每周一 9:30`
- `monthly@1@9:00` or `每月1日 9:00`
- Raw 6-field cron: `0 30 9 * * *` (sec min hour dom mon dow)

LIST

  cron_job(action="list")

ENABLE / DISABLE / DELETE

  cron_job(action="disable", job_id="cron-abc123def456")
  cron_job(action="enable", job_id="cron-abc123def456")
  cron_job(action="delete", job_id="cron-abc123def456")

After create, tell the user the **schedule**, **next run time**, and that they
can also manage jobs under **Settings → Automation**.
