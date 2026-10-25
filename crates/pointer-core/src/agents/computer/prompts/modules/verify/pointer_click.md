### Scenario: pointer click

Two questions in order. Stop at the first one that decides it.

**Evidence:** **[Screen before action]** vs **[Screen after action]**.

### Step 1 — where did the change happen?

Compare the two frames and find the region that differs.

**When `goal` names a position** — a row and column, an ordinal, a named panel —
that region must be **at that position**. A change of the right kind in the wrong
cell, the wrong row, or the wrong panel belongs in the second case below.

- **At the element `goal` names** → go to step 2.
- **At a different element, while the one `goal` names is unchanged** → `fail`,
  `wrong_operation`. The click reached something else.
- **Nothing differs anywhere** → `fail`, `wrong_operation`. The click reached the
  element and nothing happened.

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

**`loading_detected`:** spinner, progress bar, or partial load in after.

### Reasoning (hard rule)

Max **3 sentences**, plain prose.

**Forbidden:** "Analyze the Request"; JSON/`{`/`}`; repeating `submit_verify` args.
