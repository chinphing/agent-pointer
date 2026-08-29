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

Inspect, wait for, or cancel **background jobs** started with
`run_subagent` `background: true`.

Jobs keep running after this turn ends.
Do not tell the user everything is finished while jobs are still running.

**`list`**

- This conversation's background jobs.
- Includes `runningCount` and `slotCap` so you can refill free slots.

**`status`**

- One job. Requires **`jobId`**.
- Terminal jobs include truncated `content`.

**`await`**

- Wait for jobs to finish. Does **not** kill them on timeout.
- **`jobIds`**: omit = jobs this parent still has running.
- **`mode`**: `any` (default) = return the first terminal result;
  others keep running. `all` = wait until this set is finished.
- **`timeoutMs`**: default 30 minutes.
- Use `any` when the next task depends on a finished result.
- Use `all` when the full set is already spawned and you only need the summary.

**`cancel`**

- **`jobIds`**: omit = cancel every background job in this conversation.

Do not call `await` just to idle; if this turn can end, end it
and leave jobs running.
