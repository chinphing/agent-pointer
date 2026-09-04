---
schema:
  type: object
  properties:
    action:
      type: string
      enum:
        - list
        - status
        - await
        - cancel
    jobId:
      type: string
    jobIds:
      type: array
      items:
        type: string
    mode:
      type: string
      enum:
        - any
        - all
    timeoutMs:
      type: number
  required:
    - action
  additionalProperties: true
---

### `job`

Inspect, wait for, or cancel **background jobs**.
Two kinds — do not mix them up:

- **`kind: "subagent"`** — a worker from `run_subagent` `background: true`.
  `content` is the worker's Markdown handoff. `agentId` is set.
- **`kind: "terminal"`** — a shell command from `terminal` `blockUntilMs`.
  `content` is command JSON (`stdout`, `stderr`, `exitCode`).
  No `agentId`. This is not a subagent.

Jobs keep running after this turn ends.
Do not tell the user everything is finished while jobs are still running.

**`list`**

- This conversation's background jobs.
- Includes `runningCount`, `slotCap`, `idleSlots`, and `poolRunning`.
- `runningCount` = background jobs still queued or running
  (sidebar occupancy).
- `poolRunning` = root worker slots held now
  (foreground join + background share this pool).
- `idleSlots` = `slotCap - poolRunning - waiters`
  (how many more workers can start immediately).
  Foreground join also consumes pool slots — do not treat
  `idleSlots` as background-only capacity.
- Each job has **`claimed`**.
  `true` = **`await` already delivered** that body to you.
  A later `await` will not put it in `jobs` again.
  `false` + finished = stored, not yet delivered.
- Metadata only: `status`, `kind`, `claimed`, `title`, `error`.
  **No `content`.** Do not use list to read bodies.

**`status`**

- One job. Requires **`jobId`**.
- Same metadata as list. **No `content`.** Does **not** claim.
- If this turn needs the body, **`await`** the id.

**`await`**

- The **only** way a job body enters this turn.
- Also drains mid-flight **`updates[]`** (progress / status mail).
- Wait for jobs to finish **or** for progress mail. Does **not**
  kill them on timeout.
- **`jobIds`**: omit = every background job in this conversation.
  Finished unclaimed jobs are eligible immediately.
- **`mode`**: `any` (default) = wake when **either**
  (1) at least one job is finished and unclaimed, **or**
  (2) a watched job posts progress mail (tool running mid-flight).
  On wake: return every currently finished unclaimed body in this
  conversation in `jobs[]` (claimed), **and** drain mailbox into
  `updates[]`. Progress-only wake leaves `jobs` empty and does
  **not** claim bodies — call `await` again for the handoff.
  Jobs still running stay in `running`.
  Already-claimed jobs are omitted from `jobs`.
  `all` = wait until this set is finished, then return and claim
  every still-unclaimed finished job in this conversation
  (including ones outside this set). Also drains `updates[]`.
  Already-claimed jobs are omitted. Finished unclaimed jobs do
  not rerun.
- **`timeoutMs`**: default 30 minutes.
  On timeout: no body claim; any pending mail still drains into
  `updates[]`.
- Use `any` when the next task depends on a finished result,
  or when you want mid-flight progress before the handoff.
- Use `all` when the full set is already spawned and you only
  need the summary.
- `jobs[]` is the bodies. `updates[]` is mid-flight mail
  (`kind`: `progress` / `status`). `running[]` is ids still running.
  `runningCount` is background occupancy.
  `idleSlots` / `poolRunning` are the shared worker pool
  (foreground join counts against the same cap).
  Spawn at most `idleSlots` more if work remains.
  Do not `status` a finished job for the body after `await`.
  Do not treat progress `updates` as a finished handoff.

**`cancel`**

- **`jobIds`**: omit = cancel every background job in this conversation.

Do not call `await` just to idle; if this turn can end, end it
and leave jobs running.
When you already ended the turn, the host later delivers unclaimed
Completed/Failed bodies in the same conversation as one user message
starting with the Chinese line for finished background work.
Do not `await` those ids again (already claimed).
Cancelled jobs are not delivered this way.
