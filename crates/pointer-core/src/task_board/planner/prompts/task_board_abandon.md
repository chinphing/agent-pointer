Close the **current** board when the user **switched to unrelated work** and the visible board is still unfinished.

**When to call**
- Latest user message targets a **different outcome** than the board **`goal`**
  (different app, site, workflow, or deliverable).
- The board is still **running** with incomplete milestones or work_items.

**When not to call**
- User continues the **same** campaign (including short replies like “continue”, “ok”).
- Board is already terminal.

**Effect**
- Host marks the board **failed** and stops injecting it into execution.
- Then call **`task_board_init`** in the same pass when the new request is multi-step.
- For a **single-step** new request, **abandon only** — no init.

Do **not** ask the user to confirm a switch when the new request is already explicit.
