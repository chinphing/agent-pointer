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

**Scope (read first)**

Every action sees only **your own subtree**:
the jobs **you** started plus the jobs started by **your sub-agents**.
The lead sees the whole conversation.

- You do **not** see the job you are running inside, your parent's jobs,
  or a sibling worker's jobs.
- An id you cannot see is not yours: do **not** retry it, do not guess.
  Passing it explicitly is a hard error (see `await`).
- Job ids from your `run_subagent` handoff (`openBackgroundJobs`) and from
  your own `terminal` calls are always in scope.

**`list`**

- Background jobs in **your scope** (see above).
- Includes `runningCount`, `slotCap`, `idleSlots`, and `poolRunning`.
  These four are **conversation-wide** occupancy numbers, not scope counts.
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
- A job outside your scope returns `ERROR` — it is not yours to inspect.
- If this turn needs the body, **`await`** the id.

**Await vs end turn (read first)**

Default: **end your turn** — do not await. The host delivers finished
results as a new user message. Await only when your reply or the next
tool call is blocked on a specific result.

**`await`**

- The **only** way a job body enters this turn.
- Wait until a **whole job** finishes. Inner tool calls
  on a still-running worker do **not** complete this await.
- Does **not** kill jobs on timeout.
- **`jobIds`**: omit = every background job in **your scope**.
  Finished unclaimed jobs are eligible immediately.
  Passing an id outside your scope is `ERROR` — that job belongs to
  your parent, a sibling, or an ancestor.
- **`mode`**: `any` (default) = wake when at least one job
  **in your scope** is finished and unclaimed.
  On wake: return every currently finished unclaimed body
  in `jobs[]` (claimed). Jobs still running stay in `running`.
  Already-claimed jobs are omitted from `jobs`.
  `all` = wait until this set is finished, then return and claim
  every still-unclaimed finished job **in your scope**
  (including ones outside this set).
  Already-claimed jobs are omitted. Finished unclaimed jobs do
  not rerun.
- **`timeoutMs`**: default 30 minutes.
  On timeout: no body claim.
- Use `any` when the next task depends on a finished result.
- Use `all` when the full set is already spawned and you only
  need the summary.
- A worker that omitted `jobIds` with **no sub-jobs** gets
  `jobs: []` immediately — it never waits for itself.
- `jobs[]` is the bodies. `running[]` is ids still running.
  `runningCount` is background occupancy.
  `idleSlots` / `poolRunning` are the shared worker pool
  (foreground join counts against the same cap).
  Spawn at most `idleSlots` more if work remains.
  Do not `status` a finished job for the body after `await`.

**`cancel`**

- **`jobIds`**: omit = cancel every background job **in your scope**.
- Explicit ids outside your scope are **skipped** and listed in
  `denied[]`; everything else in the call still cancels.
- A sub-agent never cancels the lead's jobs or a sibling's jobs.

Do not call `await` just to idle; if this turn can end, end it
and leave jobs running.
When you already ended the turn, the host later delivers unclaimed
Completed/Failed bodies in the same conversation as one user message
starting with the Chinese line for finished background work.
Do not `await` those ids again (already claimed).
Cancelled jobs are not delivered this way.
