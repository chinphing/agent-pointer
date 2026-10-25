### Scenario: pointer click

Two questions in order. Stop at the first one that decides it.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

### Step 1 — where did the click land?

The before frame carries the pointer at the click point, and the target `goal`
names is intact in that frame. Compare the pointer against the target's bounds.

- **Pointer inside the target** → go to step 2.
- **Pointer outside it** — the neighbouring cell, the row above or below, the
  adjacent button → `fail`, `precision_miss`. The click missed; the planner
  re-aims from this.
- **Somewhere unrelated** — a different panel, a different part of the screen →
  `fail`, `wrong_operation`. The click reached the wrong element.
- **No pointer visible in the before frame** → judge from where the change
  appeared, below.

### Step 2 — did the expected change happen?

Work through the target in order: what it is in the before frame, what it is in
the after frame, what differs. Only a difference counts as a change — a value
already present in the before frame did not appear because of this click.

The change does not have to be on the element itself. A save button looks the
same before and after; the toast, the dialog, the new page, the row that
disappeared — that is the change. But it must be a **difference between the two
frames**, not a state you notice in the after frame alone.

- **The change `goal` expects** → `pass`.
- **Some change, but not the expected one** — highlighted instead of entered,
  focused instead of opened, the wrong menu expanded → `fail`, `wrong_operation`.
- **No change anywhere** → `fail`, `wrong_operation`. The pointer was on the
  target and nothing happened.

**`loading_detected`:** spinner, progress bar, or partial load in after.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

**Forbidden:** "Analyze the Request"; JSON/`{`/`}`; repeating `submit_verify` args.
