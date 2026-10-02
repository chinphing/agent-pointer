# Scheduled tasks

English | [简体中文](../../zh-CN/user/scheduled-tasks.md)

Scheduled tasks let an **agent run a chat turn on a schedule** — when the time comes it runs the prompt by itself, and when it is done it can push the result to IM.

It is not a cron that "runs one command": every trigger is a full chat turn, so the prompt can ask it to look things up, read files and call tools, and then report back in natural language.

```
Scheduler (checks about once a minute)
   └─ Time reached → run a turn in the job's own chat
        ├─ Chat id: cron:{job id}:{date}   ← this is what "View chat" opens
        └─ Push ticked → send this turn's final reply to the chosen IM channel
```

## Two entry points

| You want to | Go to |
| --- | --- |
| Build it in the UI and manage it from the list | **Settings → Automation → Scheduled tasks** |
| Just say it in passing | Say "check XX for me every day at 9am" in the chat; the assistant creates it with the `cron_job` tool |

The `cron_job` tool in chat supports five actions: **create / list / enable / disable / delete**. The UI and the tool share one data set — jobs the assistant created show up in Settings, and you can ask it to list the jobs you created in the UI.

## Creating a scheduled task

**Settings → Automation → Scheduled tasks → New**

| Field | Description |
| --- | --- |
| **Name** | The name shown in the list, for example "Daily digest" |
| **Agent** | Which executing agent runs it (General / Vibe coding / Computer use); General by default |
| **Trigger prompt** | The prompt sent to the agent on every trigger. **This is the body of the job** — spell out what to do and how to report |
| **Schedule** | Pick the frequency with the selector below |
| **Push to IM** | When ticked, multi-select from bound channels; leave it unticked to keep the result in the chat only |

The schedule selector covers the vast majority of cases; you never hand-write an expression:

| Selector option | Generated expression |
| --- | --- |
| Once after a delay | `30m` / `2h` / `1d` |
| Once at a set time | `2026-07-22T09:00:00` |
| Every minute | `0 * * * * *` |
| Every N minutes | `0 */5 * * * *` |
| Every N hours | `0 0 */2 * * *` |
| Daily | `0 30 9 * * *` (9:30) |
| Weekly | `0 30 9 * * 1` (Monday 9:30) |
| Monthly | `0 0 9 1 * *` (day 1 at 9:00) |
| Custom | Use your string as written |

### Other ways to write a time

When you ask the assistant to create a job in chat, the `schedule` field accepts these forms:

| Form | Example | Meaning |
| --- | --- | --- |
| Relative | `30m`, `2h`, `1d` | Run once 30 minutes / 2 hours / 1 day from now |
| ISO time | `2026-07-22T09:00:00` | Run once at a set local time (also accepts `2026-07-22 09:00:00`) |
| Daily | `daily@9:30`, `每天 09:00` | A fixed time every day |
| Weekly | `weekly@1@9:30`, `每周一 9:30` | 0=Sunday … 6=Saturday |
| Monthly | `monthly@1@9:00`, `每月1日 9:00` | A fixed day every month |
| Every minute / every N minutes | `every_minute`, `every_5_minutes`, `每10分钟` | Recurring |
| Every N hours | `every_2_hours`, `每2小时` | Recurring |
| **6-field cron** | `0 30 9 * * *` | Write the expression directly |

> **The first field of a 6-field cron must be `0`.** Pointer only accepts 6-field expressions whose seconds field is `0`; cron macros such as `@daily` and the common 5-field cron (`30 9 * * *`) are not recognised. If you cannot write one, use the friendly forms above.

## Job list

Each row, top to bottom:

| Position | Content |
| --- | --- |
| Toggle on the left | Tooltip "Enabled (click to disable)" / "Disabled (click to enable)" |
| Main line | `Name · Agent · Schedule`, plus the delivery channel when push is ticked |
| Right side | "Next {time}" |
| Actions | "Edit delivery target" ("Set delivery target" when never set), "View chat", delete |
| Error | On a delivery failure, an extra line "Delivery failed: {reason}" |

Deleting takes **two steps**: click the bin once and the button turns red with the title "Confirm delete" and a "Cancel" beside it; click again to actually delete.

## Every run is a chat

Every trigger runs a turn in its own chat. The chat id is `cron:{job id}:{date}` — so the records of one job on different days are separate.

Clicking "**View chat**" on the row opens that run's chat directly, where you can see what it thought at the time and which tools it called. Before the job has ever triggered the button is greyed out, saying "Not triggered yet; no chat to open".

## Pushing to IM

Once "Push to IM" is ticked and channels are chosen, every run sends its **final reply** to the chosen channels.

- The channel must **already be bound**: you need to have messaged Pointer privately in that channel first
- One channel maps to **the person who last messaged Pointer privately**
- To skip the push for one run, have the reply include `[SILENT]`

## The scheduler itself

Scheduling is handled by the scheduler inside the app, **on by default**, with no UI toggle. The only way to turn it off is an environment variable:

```bash
POINTER_SCHEDULER_ENABLED=0
```

Both the server and the desktop default to on; once it is off no scheduled task triggers any more (the jobs themselves remain).

The scheduler **checks about once a minute**, so a one-shot job may actually trigger up to a minute after the set time.

When a one-shot job finishes it is **soft-completed**: it is not deleted (history is kept), but it becomes disabled and cannot be re-enabled.

## FAQ

**"Enter a name" / "Enter a schedule" / "Enter a trigger prompt"**

All three are required in the create form.

**"Select at least one delivery channel"**

"Push to IM" is ticked but no channel is selected.

**"No enabled IM channels." / "Message Pointer in that channel to bind, then choose it."**

No channel is available yet. Go to **Settings → Connections** to enable one, and message Pointer privately in that channel to bind it.

**"One-shot jobs cannot be re-enabled; create a new one"**

A one-shot job is soft-completed once it has run: history is kept but it never triggers again. Create a new one to run it again.

**"This job has not run; no dedicated chat yet"**

The job has not reached its time yet, so there is no chat. Wait for one trigger, or first check whether the toggle is disabled.

**The assistant says it does not recognise the time format**

Rewrite it in a friendly form. On a parse failure the examples it gives are: `30m`, `2026-07-22T09:00:00`, `daily@9:30`, `every_5_minutes`, `0 30 9 * * *`.

**It did not run at the set time**

Check three things in order: the job's enable toggle, whether the scheduler was turned off with `POINTER_SCHEDULER_ENABLED=0`, and whether a one-shot job has already run (soft-completed).

## Related

- [Settings overview](settings.md) — the Automation section and execution limits
- [IM channels](im-channels.md) — connect a channel first so pushes have somewhere to go
- [Webhook-triggered agents](webhook.md) — triggered by an external event instead of by time
