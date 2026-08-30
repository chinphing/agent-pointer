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
- Includes `runningCount`, `slotCap`, and `idleSlots`
  (`slotCap - runningCount`) so you can refill free slots.
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
- Wait for jobs to finish. Does **not** kill them on timeout.
- **`jobIds`**: omit = every background job in this conversation.
  Finished unclaimed jobs are eligible immediately.
- **`mode`**: `any` (default) = wait until at least one job is
  finished and unclaimed, then return **every** currently finished
  unclaimed job in this conversation — each with full `content` —
  and mark them claimed. Jobs still running stay in `running`.
  Do not wait for the slowest. Finished jobs that were not yet
  claimed are returned from storage; they do not rerun.
  Already-claimed jobs are omitted from `jobs`.
  `all` = wait until this set is finished, then return and claim
  every still-unclaimed finished job in this conversation
  (including ones outside this set). Already-claimed jobs are
  omitted. Finished unclaimed jobs do not rerun.
- **`timeoutMs`**: default 30 minutes.
- Use `any` when the next task depends on a finished result.
- Use `all` when the full set is already spawned and you only need the summary.
- `jobs[]` is the bodies. `running[]` is ids still running.
  `runningCount` / `idleSlots` are this conversation
  (queued + running). Spawn `idleSlots` more if work remains.
  Do not `status` a finished job for the body after `await`.

**`cancel`**

- **`jobIds`**: omit = cancel every background job in this conversation.

Do not call `await` just to idle; if this turn can end, end it
and leave jobs running.
